use super::*;
#[test]
fn probe_requires_exact_magic_version_and_atomic_snapshot_capability() {
    let mut regs = [0u32; 8];
    let base = regs.as_ptr() as usize;
    assert!(unsafe { Controller::probe(base) }.is_none());
    regs[0] = MAGIC;
    regs[1] = 1;
    regs[4] = 1;
    assert!(unsafe { Controller::probe(base) }.is_some());
    regs[1] = 2;
    assert!(unsafe { Controller::probe(base) }.is_none());
    regs[1] = 1;
    regs[4] = 0;
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
