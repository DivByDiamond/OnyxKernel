use super::{
    globals::{G_ALL_PROCS, G_HART_CURRENT, hart_id},
    types::{PROC_RING_KERNEL, Proc, ProcState},
};

pub fn current_pid() -> u32 {
    // SAFETY: reads this hart's own G_HART_CURRENT slot (written only by the
    // owning hart's scheduler); non-null points at a live heap-allocated Proc.
    unsafe {
        let p = G_HART_CURRENT[hart_id()];
        if p.is_null() {
            return 0;
        }
        (*p).pid
    }
}

pub fn current_ring() -> u8 {
    // SAFETY: same discipline as current_pid (own-hart slot, live Proc).
    unsafe {
        let p = G_HART_CURRENT[hart_id()];
        if p.is_null() {
            return PROC_RING_KERNEL;
        }
        (*p).ring
    }
}

/// Owned metadata: callers can retain this value across mutation, scheduling,
/// or process teardown. It contains no references or dereferenceable pointers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CurrentProcessSnapshot {
    pub pid: u32,
    pub ring: u8,
    pub root_pa: u64,
    pub uid: u32,
    pub gid: u32,
    pub stack_canary: u64,
}

/// Return copied metadata rather than an unconstrained mutable Proc reference.
/// Disable local interrupts during the copy so this hart cannot switch away
/// between reading its current slot and the last field. No Proc borrow escapes.
pub fn current_opt() -> Option<CurrentProcessSnapshot> {
    // SAFETY: current slots are owned by their hart. With local interrupts
    // disabled, a current Proc cannot be switched out/reaped during this copy.
    // Raw field reads avoid creating a whole-Proc reference that could alias
    // references already held by syscall callers.
    unsafe {
        use crate::arch::{csr, regs::SSTATUS_SIE};
        let status = csr::read_sstatus();
        csr::clear_sstatus(SSTATUS_SIE);
        // CSR helpers use asm(nomem): explicitly keep memory accesses inside
        // the interrupt-disabled region at the compiler level as well.
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
        let p = G_HART_CURRENT[hart_id()];
        let snapshot = if p.is_null() {
            None
        } else {
            Some(snapshot_from_ptr(p))
        };
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
        if status & SSTATUS_SIE != 0 {
            csr::set_sstatus(SSTATUS_SIE);
        }
        snapshot
    }
}

/// # Safety
/// `p` must remain live and its metadata stable for this copy. The current
/// accessor enforces this through hart ownership and disabled interrupts.
unsafe fn snapshot_from_ptr(p: *const Proc) -> CurrentProcessSnapshot {
    // SAFETY: only Copy fields are read through a live pointer; no &Proc or
    // &mut Proc is created, and stack bytes are read without alignment assumptions.
    unsafe {
        CurrentProcessSnapshot {
            pid: core::ptr::read(&raw const (*p).pid),
            ring: core::ptr::read(&raw const (*p).ring),
            root_pa: core::ptr::read(&raw const (*p).root_pa),
            uid: core::ptr::read(&raw const (*p).uid),
            gid: core::ptr::read(&raw const (*p).gid),
            stack_canary: core::ptr::read_unaligned((&raw const (*p).kstack).cast::<u64>()),
        }
    }
}

/// # Safety
///
/// Caller contract: must run in process context on this hart (current is
/// non-null); returns a reference to the per-hart current Proc, valid only
/// while it stays current on this hart. The caller must ensure exclusive
/// access for the entire borrow, including callees: do not call current()
/// again, yield, or retain this reference across a context switch. Prefer
/// current_opt() for read-only metadata; it returns an owned snapshot.
pub unsafe fn current() -> &'static mut Proc {
    // SAFETY: caller contract guarantees the slot is non-null and points at
    // a live heap-allocated Proc that stays current on this hart.
    unsafe {
        let p = G_HART_CURRENT[hart_id()];
        &mut *p
    }
}

/// # Safety
///
/// Caller contract: process context on this hart; writes only this
/// process's own fixed-size cwd buffer.
pub unsafe fn set_cwd(path: &[u8]) {
    // SAFETY: current() is valid per the caller contract; copy length is
    // clamped to the 256-byte cwd buffer (n <= 255 leaves room for NUL).
    unsafe {
        let p = current();
        let n = path.len().min(255);
        p.cwd[..n].copy_from_slice(&path[..n]);
        p.cwd[n] = 0;
        p.cwd_len = n as u16;
    }
}

pub fn cwd() -> &'static [u8] {
    // SAFETY: same as current_pid; slice is bounded by cwd_len <= 255 inside
    // the fixed 256-byte cwd array.
    unsafe {
        let p = current();
        &p.cwd[..p.cwd_len as usize]
    }
}

/// Find a live (non-`Free`) process by pid.
///
/// Takes `proc_list_lock` internally, so it is safe to call from any context
/// that does NOT already hold that lock (audited: all syscall/trap/signal
/// call sites qualify). The returned reference is only protected during the
/// traversal — mutate the fields per each caller's own locking discipline as
/// before.
///
/// callers already holding `proc_list_lock` must use [`by_pid_unlocked`].
///
/// # Safety
///
/// Caller contract: must NOT already hold `proc_list_lock` (self-deadlock);
/// the returned reference stays valid until the process is reaped (kfree
/// under proc_list_lock in wait()).
pub unsafe fn by_pid(pid: u32) -> Option<&'static mut Proc> {
    // SAFETY: list traversal happens entirely under proc_list_lock taken
    // here; nodes are unlinked/freed only while holding the same lock.
    unsafe {
        super::globals::proc_list_lock();
        let found = by_pid_unlocked(pid);
        super::globals::proc_list_unlock();
        found
    }
}

/// Unlocked variant of [`by_pid`] for contexts that already hold
/// `proc_list_lock` (e.g. the exit path's burst state update). Calling this
/// without the lock held is a data-race bug against concurrent list mutation.
///
/// # Safety
///
/// Caller contract: proc_list_lock MUST already be held; otherwise the
/// traversal races with concurrent unlink/reap of list nodes.
pub unsafe fn by_pid_unlocked(pid: u32) -> Option<&'static mut Proc> {
    // SAFETY: per the caller contract, proc_list_lock is held, excluding
    // concurrent list mutation while nodes are dereferenced.
    unsafe {
        let mut cur = G_ALL_PROCS;
        while !cur.is_null() {
            if (*cur).pid == pid && !matches!((*cur).state, ProcState::Free) {
                return Some(&mut *cur);
            }
            cur = (*cur).all_next;
        }
        None
    }
}

pub fn dump_all<W: onyx_core::fmt::Write>(w: &mut W) {
    // Waitpid-race follow-up (todo P1 #2): the traversal now runs under
    // proc_list_lock so it cannot race concurrent unlink/reap. try_lock
    // (not lock) because dump_all is also invoked from the panic path
    // (srv::klog), which may fire while the lock is already held — then we
    // degrade to a best-effort unlocked read instead of deadlocking the
    // panic handler.
    //
    // Root-cause fix (SMP crash, todo.md "Отдельный SMP-краш под
    // -smp 2"): the "best-effort unlocked read" used to walk
    // `G_ALL_PROCS`/`(*cur).all_next` anyway when the lock wasn't held —
    // but another hart concurrently unlinking/relinking a node (exit()'s
    // auto-reap unlink, publish_ready's list insert, ...) can leave `cur`
    // pointing at memory that's mid-mutation or already freed, and walking
    // stale/dangling links wanders into unrelated memory. Live testing
    // confirmed this: panic-time process dumps occasionally printed
    // nonsensical mixed text — fragments of a completely unrelated, much
    // earlier boot message ("stvec=%p") interleaved with garbage where a
    // `pid=%d state=%s ...` line should have been, meaning `vformat` was
    // fed corrupted args or even a corrupted format-string pointer read
    // from wherever the dangling walk had wandered. Diagnostics reading
    // garbage and got risk crashing again themselves; skip the walk
    // entirely when the lock isn't held instead of guessing at how far a
    // "best-effort" unlocked read can safely go.
    let locked = super::globals::G_PROC_LIST_LOCK.try_lock();
    if !locked {
        w.write_str("  (process list locked elsewhere, dump skipped)\n");
        return;
    }
    // SAFETY: proc_list_lock is held (checked above), so the list cannot be
    // mutated by another hart while we walk it.
    unsafe {
        let mut cur = G_ALL_PROCS;
        while !cur.is_null() {
            if !matches!((*cur).state, ProcState::Free) {
                let state_str = match (*cur).state {
                    ProcState::Ready => "Ready",
                    ProcState::Running => "Running",
                    ProcState::Exited => "Exited",
                    ProcState::Waiting => "Waiting",
                    ProcState::Creating => "Creating",
                    ProcState::Stopped => "Stopped",
                    ProcState::Free => "Free",
                };
                let args: &[onyx_core::fmt::Arg] = &[
                    onyx_core::fmt::Arg::from((*cur).pid),
                    onyx_core::fmt::Arg::from(state_str),
                    onyx_core::fmt::Arg::from((*cur).ring as u32),
                    onyx_core::fmt::Arg::from((*cur).parent_pid),
                ];
                onyx_core::fmt::vformat(w, "    pid=%d state=%s ring=%d ppid=%d\n", args);
            }
            cur = (*cur).all_next;
        }
    }
    super::globals::proc_list_unlock();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_snapshot_outlives_process_without_borrowing_it() {
        let mut process = Box::new(Proc::new());
        process.pid = 42;
        process.uid = 1000;
        process.gid = 100;
        process.ring = 2;
        process.parent_pid = 7;
        process.root_pa = 0x123000;
        process.kstack[..8].copy_from_slice(&super::super::types::KSTACK_CANARY.to_ne_bytes());
        // SAFETY: local live Proc, no concurrent accesses; no reference escapes.
        let snapshot = unsafe { snapshot_from_ptr(&raw const *process) };
        process.uid = 2000;
        drop(process);
        assert_eq!(snapshot.pid, 42);
        assert_eq!(snapshot.uid, 1000);
        assert_eq!(snapshot.gid, 100);
        assert_eq!(snapshot.ring, 2);
        assert_eq!(snapshot.root_pa, 0x123000);
        assert_eq!(snapshot.stack_canary, super::super::types::KSTACK_CANARY);
    }

    #[test]
    fn optional_current_is_an_owned_send_sync_value() {
        fn assert_owned<T: Copy + Send + Sync + 'static>(_: fn() -> Option<T>) {}
        assert_owned(current_opt);
    }
}
