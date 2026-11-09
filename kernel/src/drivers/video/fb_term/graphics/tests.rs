use super::*;
#[test]
fn a_claim_is_exclusive_idempotent_and_only_its_owner_can_release() {
    let owner = Owner::new();
    assert_eq!(owner.claim(0), Err(Errno::Inval));
    assert!(owner.claim(7).is_ok());
    assert!(owner.claim(7).is_ok());
    assert_eq!(owner.claim(8), Err(Errno::Busy));
    assert!(!owner.release(8));
    assert_eq!(owner.0.load(Ordering::Acquire), 7);
    assert!(owner.release(7));
    assert!(!owner.release(7));
    assert!(owner.claim(8).is_ok());
}
