use super::*;

/// Combined test: NET_LOCK is a process-global static and the host test
/// harness runs #[test] fns in parallel, so both nesting and
/// release/reacquire assertions live in one function to stay
/// deterministic (same pattern as runqueue.rs).
#[test]
fn test_recursive_lock_nesting_and_reacquire() {
    let _serial = crate::net::lock::test_serial();
    // Host test: hart_id() == 0, single thread — exercise the recursion.
    net_lock();
    assert_eq!(owned_depth(), 1);
    net_lock();
    net_lock();
    assert_eq!(owned_depth(), 3);
    // Depth must unwind exactly; the underlying lock is only free at 0.
    net_unlock();
    net_unlock();
    assert_eq!(owned_depth(), 1);
    net_unlock();
    assert_eq!(owned_depth(), 0);
    // Fully released — acquire again cleanly.
    net_lock();
    assert_eq!(owned_depth(), 1);
    net_unlock();
    assert_eq!(owned_depth(), 0);
}
