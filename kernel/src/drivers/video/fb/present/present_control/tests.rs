use super::*;
#[test]
fn probe_requires_exact_magic_version_and_atomic_snapshot_capability() {
    let mut regs = [0u32; 8];
    let base = regs.as_mut_ptr() as usize;
    let set = |index: usize, value: u32| {
        assert!(index < 8);
        // SAFETY: index is bounded and base points to the live aligned array.
        unsafe { core::ptr::write_volatile((base as *mut u32).add(index), value) };
    };
    // SAFETY: base covers eight initialized fake MMIO registers.
    assert!(unsafe { Controller::probe(base) }.is_none());
    set(0, MAGIC);
    set(1, 1);
    set(4, 1);
    // SAFETY: the same live register array remains valid.
    assert!(unsafe { Controller::probe(base) }.is_some());
    set(1, 2);
    // SAFETY: the same live register array remains valid.
    assert!(unsafe { Controller::probe(base) }.is_none());
    set(1, 1);
    set(4, 0);
    // SAFETY: the same live register array remains valid.
    assert!(unsafe { Controller::probe(base) }.is_none());
}
#[test]
fn rejected_commit_cancels_instead_of_leaving_legacy_apps_frozen() {
    let mut regs = [0u32; 8];
    regs[3] = ACTIVE | NEGOTIATED;
    let p = Publication {
        controller: Some(Controller(regs.as_mut_ptr() as usize)),
    };
    assert!(!p.commit());
    assert_eq!(regs[2], DISABLE);
    let p = Publication {
        controller: Some(Controller(regs.as_mut_ptr() as usize)),
    };
    drop(p);
    assert_eq!(regs[2], DISABLE);
}
