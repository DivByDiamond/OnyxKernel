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
