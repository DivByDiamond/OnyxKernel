//! Unit tests for the xHCI class filter (pure logic, no MMIO).
use super::is_xhci;

#[test]
fn xhci_matches_class_subclass_and_prog_if() {
    assert!(is_xhci(0x0C03, 0x30));
    // USB 2.0 EHCI (0x20) / OHCI (0x10) are USB but not xHCI.
    assert!(!is_xhci(0x0C03, 0x20));
    assert!(!is_xhci(0x0C03, 0x10));
    // Same prog-if under a different class/subclass must not match.
    assert!(!is_xhci(0x0C00, 0x30));
    assert!(!is_xhci(0x0300, 0x30));
}
