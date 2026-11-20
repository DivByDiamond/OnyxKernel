use super::{
    dev, endpoint_is_in, interrupt_transfer,
    ports::{owner_n_ports, owner_port_enable, owner_port_reset, owner_port_status},
};
use onyx_core::errno::Errno;

/// Registry slot this test may use. The dev registry test owns slot 0
/// and the drain-routing test (event/tests.rs) owns slot 1; slot 2
/// stays private to these dispatch tests, so parallel test harts
/// cannot interfere.
const DISPATCH_SLOT: usize = 2;

/// An active slot for `DISPATCH_SLOT` with the given owner.
fn dispatch_slot(owner: dev::Owner) -> dev::UsbSlot {
    dev::UsbSlot {
        active: true,
        owner,
        addr: 1,
        vendor: 0,
        product: 0,
        class: 0, // no HID session attaches in host tests, so no drain path routes here
        iface: 0,
        ep_in: 0x81,
        max_packet: 8,
        interval: 10,
    }
}

/// Deactivate `DISPATCH_SLOT` again (free for the next pass).
fn clear_slot() {
    let mut inactive = dispatch_slot(dev::Owner::Ohci);
    inactive.active = false;
    assert_eq!(dev::set_slot(DISPATCH_SLOT, inactive), Ok(()));
}

#[test]
fn endpoint_direction_follows_address_bit_7() {
    // OUT endpoints: EP0, EP1, EP15 — host → device.
    assert!(!endpoint_is_in(0x00));
    assert!(!endpoint_is_in(0x01));
    assert!(!endpoint_is_in(0x0F));
    // IN endpoints: EP1..EP15 with the direction bit set — device → host.
    assert!(endpoint_is_in(0x81));
    assert!(endpoint_is_in(0x8F));
    // Only bit 7 matters: the endpoint number must not leak into direction.
    assert!(!endpoint_is_in(0x7F));
}

#[test]
fn interrupt_transfer_dispatches_by_slot_owner() {
    let mut buf = [0u8; 8];
    // Inactive or out-of-range slot: NoEnt, nothing dispatched.
    assert_eq!(
        unsafe { interrupt_transfer(dev::MAX_SLOTS, &mut buf) },
        Err(Errno::NoEnt)
    );
    // OHCI slot routes to the OHCI poll (ruling 5): on the host the
    // channel was never armed, so the poll guard reports NoEnt — a
    // plain static read, no MMIO. EHCI-owned interrupt slots stay
    // NoSys (ruling 1: EHCI interrupt transfers out of scope).
    assert_eq!(
        dev::set_slot(DISPATCH_SLOT, dispatch_slot(dev::Owner::Ohci)),
        Ok(())
    );
    assert_eq!(
        unsafe { interrupt_transfer(DISPATCH_SLOT, &mut buf) },
        Err(Errno::NoEnt)
    );
    clear_slot();
    // An xHCI-owned slot reaches the xHCI poll, which on the host bails
    // at the not-operational guard (a plain static read, no MMIO).
    assert_eq!(
        dev::set_slot(DISPATCH_SLOT, dispatch_slot(dev::Owner::Xhci)),
        Ok(())
    );
    assert_eq!(
        unsafe { interrupt_transfer(DISPATCH_SLOT, &mut buf) },
        Err(Errno::NoSys)
    );
    clear_slot();
    // With the slot deactivated again, the dispatcher is back to NoEnt.
    assert_eq!(
        unsafe { interrupt_transfer(DISPATCH_SLOT, &mut buf) },
        Err(Errno::NoEnt)
    );
}

#[test]
fn owner_port_dispatchers_reach_each_controllers_guard() {
    // Host: no controller initialized, so each owner's branch answers
    // through its inert guard without MMIO — xHCI's operational flag,
    // the OHCI/EHCI zero port counts (bounds-checked before any
    // register access) — proving the routing table itself.
    for owner in [dev::Owner::Xhci, dev::Owner::Ehci, dev::Owner::Ohci] {
        assert_eq!(owner_n_ports(owner), 0);
        assert_eq!(unsafe { owner_port_status(owner, 0) }, Err(Errno::Range));
        assert_eq!(unsafe { owner_port_reset(owner, 0) }, Err(Errno::Range));
        assert_eq!(unsafe { owner_port_enable(owner, 0) }, Err(Errno::Range));
    }
}
