//! Global resource limits — process-count caps and system-wide user-memory
//! accounting. Guards the dynamic process list against fork bombs (a runaway
//! `fork()` loop in a ring-3 process would otherwise hang the host VM thread).
//!
//! Caps chosen (all compile-time constants, see below):
//! - `MAX_PROCS` = 128 total live processes (zombies included — they still
//!   hold a heap-allocated Proc node and a PID until reaped).
//! - `MAX_PROCS_PER_UID` = 32 per non-root uid; root (`uid == 0`) gets
//!   `MAX_PROCS_ROOT_UID` = 256 so system services are not starved by user
//!   logins.
//! - `USER_MEM_MAX_BYTES` = 128 MiB system-wide budget of *user* pages.
//!   Fixed constant rather than a RAM fraction: pmm totals are not available
//!   at compile time and the guest VM is provisioned with >= 256 MiB DRAM,
//!   so 128 MiB leaves headroom for kernel heap + page tables while still
//!   bounding any single runaway allocator (per-process brk ceiling is
//!   already 64 MiB via USER_HEAP_SIZE).
//!
//! User-page accounting hooks:
//! - incremented in `vmm::map_anon_impl` (mmap), `ensure_heap_pages` (brk
//!   growth), `onx::load` stack/heap pre-map and `map_segment_data`
//!   (executable segments);
//! - decremented in `vmm::unmap_impl` (munmap / brk shrink) and
//!   `free_subtree` (address-space teardown on exit/exec).

use core::sync::atomic::{AtomicUsize, Ordering};

use onyx_core::errno::{Errno, KResult};

use super::process::{G_ALL_PROCS, PROC_RING_ROOT, ProcState, proc_list_lock, proc_list_unlock};

/// Hard cap on total live processes (any uid), zombies included.
pub const MAX_PROCS: usize = 128;
/// Per-user cap for non-root uids (fork-bomb containment).
pub const MAX_PROCS_PER_UID: u32 = 32;
/// Per-user cap for root; higher than user cap but not unlimited.
pub const MAX_PROCS_ROOT_UID: u32 = 256;
/// System-wide user-memory budget (see module docs for rationale).
pub const USER_MEM_MAX_BYTES: usize = 128 * 1024 * 1024;

const PAGE_SIZE: usize = 4096;
const USER_MEM_MAX_PAGES: usize = USER_MEM_MAX_BYTES / PAGE_SIZE;

/// Total currently-mapped user pages across all processes.
static G_USER_PAGES: AtomicUsize = AtomicUsize::new(0);

#[inline]
pub fn user_pages_in_use() -> usize {
    G_USER_PAGES.load(Ordering::Relaxed)
}

/// Map a ring value to the owning uid, mirroring the assignment done by
/// `spawn::create_user` (ring 0/1 => root, ring 2 => unprivileged user).
#[inline]
pub fn uid_for_ring(ring: u8) -> u32 {
    if ring <= PROC_RING_ROOT { 0 } else { 1000 }
}

/// Reserve one user page against the system-wide budget.
///
/// # Errors
/// Returns [`Errno::NoMem`] when the total mapped user pages would exceed
/// `USER_MEM_MAX_BYTES`. The reservation is rolled back internally on
/// failure, so callers must only release pages they actually mapped after
/// an `Ok` return.
pub fn user_page_take() -> KResult<()> {
    let prev = G_USER_PAGES.fetch_add(1, Ordering::AcqRel);
    if prev >= USER_MEM_MAX_PAGES {
        G_USER_PAGES.fetch_sub(1, Ordering::Release);
        return Err(Errno::NoMem);
    }
    Ok(())
}

/// Release `n` previously accounted user pages (saturating — never wraps).
pub fn user_pages_release(n: usize) {
    if n == 0 {
        return;
    }
    // Saturating sub: defensive against transient over-decrement from
    // non-user leaf PTEs sharing a page table subtree.
    let _ = G_USER_PAGES.fetch_update(Ordering::AcqRel, Ordering::Acquire, |v| {
        Some(v.saturating_sub(n))
    });
}

/// Number of live processes (every state except `Free`) — zombies count,
/// since each still holds a heap-allocated Proc node and a PID.
fn live_proc_count() -> usize {
    proc_list_lock();
    let mut n = 0;
    // SAFETY: G_ALL_PROCS list traversal under PROC_LIST_LOCK; nodes are
    // only unlinked/freed while holding the same lock.
    unsafe {
        let mut cur = G_ALL_PROCS;
        while !cur.is_null() {
            if !matches!((*cur).state, ProcState::Free) {
                n += 1;
            }
            cur = (*cur).all_next;
        }
    }
    proc_list_unlock();
    n
}

/// Number of live processes owned by `uid`.
fn live_proc_count_uid(uid: u32) -> u32 {
    proc_list_lock();
    let mut n = 0u32;
    // SAFETY: same locking discipline as `live_proc_count`.
    unsafe {
        let mut cur = G_ALL_PROCS;
        while !cur.is_null() {
            if !matches!((*cur).state, ProcState::Free) && (*cur).uid == uid {
                n += 1;
            }
            cur = (*cur).all_next;
        }
    }
    proc_list_unlock();
    n
}

/// Check whether one more process owned by `uid` may be created.
///
/// # Errors
/// Returns [`Errno::Again`] (EAGAIN, like POSIX fork(2)) when either the
/// global or the per-uid process cap is reached.
pub fn can_create_proc(uid: u32) -> KResult<()> {
    if live_proc_count() >= MAX_PROCS {
        return Err(Errno::Again);
    }
    let cap = if uid == 0 {
        MAX_PROCS_ROOT_UID
    } else {
        MAX_PROCS_PER_UID
    };
    if live_proc_count_uid(uid) >= cap {
        return Err(Errno::Again);
    }
    Ok(())
}
