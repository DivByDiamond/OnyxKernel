use core::hint::spin_loop;
use core::sync::atomic::{AtomicBool, Ordering};

/// Global VMM spinlock (Bug #2 fix). All page-table mutations (map, unmap,
/// split_leaf, destroy_root) go through this lock, preventing the SMP race
/// where two harts concurrently walk + mutate adjacent VA ranges and end up
/// leaking intermediate tables or producing dangling PTEs.
///
/// Read-only walkers (`translate`, `translate_user`) intentionally do NOT
/// acquire this lock — they only do `ptr::read_volatile` on PTE slots, and
/// a concurrent split_leaf may momentarily observe a stale PTE but cannot
/// corrupt the walker's state. Locking them would massively amplify lock
/// contention since translate is called from every user-pointer access.
pub(super) static G_VMM_LOCK: AtomicBool = AtomicBool::new(false);

#[inline]
pub(super) unsafe fn vmm_lock() {
    while G_VMM_LOCK.swap(true, Ordering::Acquire) {
        while G_VMM_LOCK.load(Ordering::Relaxed) {
            spin_loop();
        }
    }
}

#[inline]
pub(super) unsafe fn vmm_unlock() {
    G_VMM_LOCK.store(false, Ordering::Release);
}
