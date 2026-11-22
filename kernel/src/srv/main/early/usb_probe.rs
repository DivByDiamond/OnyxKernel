//! Boot-time USB probe: init the platform controller(s), then enumerate
//! the attached devices into the dev registry — and attach a HID
//! session (report-descriptor fetch + interrupt arm) to every HID slot.
//! Owner-routed (ruling 2) since the SG2000 branch initializes BOTH
//! controllers.

use crate::drivers::usb;
use onyx_core::errno::Errno;
use onyx_core::fmt::Arg;

/// Port-status bit 0: current-connect status — the same position in
/// EHCI PORTSC and OHCI HcRhPortStatus.
const PORT_CCS: u32 = 1 << 0;

/// QEMU exposes xHCI over PCIe (`-device qemu-xhci`) — there is no
/// generic-xhci platform node, and OC2R/sedna's ECAM is garbage; the
/// SG2000 branch inits and probes BOTH platform controllers instead
/// (unmapped on every other board, which skip USB entirely). Every
/// failure is a kwrn; boot continues.
///
/// # Safety
///
/// Must run once on the boot hart during the single-threaded peripheral
/// probe, same contract as `usb::init_usb`/`usb::xhci::init`; the
/// registry lock and the SIE=0 discipline are handled inside.
pub(crate) unsafe fn probe_usb() {
    unsafe {
        if crate::libfdt::fdt::is_qemu() {
            match crate::drivers::pcie::find_xhci() {
                Some(base) => match usb::xhci::init(base as usize) {
                    Ok(()) => {
                        crate::kinf!("usb", "xhci bar0=%p", Arg::from(base));
                        probe_xhci_ports();
                    }
                    Err(_) => {
                        crate::kwrn!("usb", "xhci init failed at bar0=%p", Arg::from(base))
                    }
                },
                None => crate::kwrn!("usb", "no xhci PCI function found"),
            }
        } else if !crate::libfdt::fdt::is_sedna() {
            // SG2000 dual init (ruling 1): EHCI for high-speed, OHCI as
            // the full/low-speed companion a HID stick rides.
            match usb::init_usb() {
                Ok(()) => {
                    crate::kinf!("usb", "host controllers initialized");
                    probe_platform();
                }
                Err(_) => crate::kwrn!("usb", "no host controller found"),
            }
        }
    }
}

/// Enumerate one device per platform controller (ruling 1): EHCI
/// (high-speed) ports first, then the OHCI (full/low-speed)
/// companion's. The slot an enumerated device lands in carries its
/// controller as owner — full-speed HID in an `Owner::Ohci` slot,
/// high-speed in `Owner::Ehci`.
unsafe fn probe_platform() {
    unsafe {
        for owner in [usb::dev::Owner::Ehci, usb::dev::Owner::Ohci] {
            if first_connected_port(owner).is_some() {
                probe_device(owner);
            }
        }
    }
}

/// First connected port on `owner`'s controller, if any.
unsafe fn first_connected_port(owner: usb::dev::Owner) -> Option<u8> {
    unsafe {
        for idx in 0..usb::ports::owner_n_ports(owner) {
            if let Ok(status) = usb::ports::owner_port_status(owner, idx)
                && status & PORT_CCS != 0
            {
                return Some(idx);
            }
        }
    }
    None
}

/// Enumerate EVERY connected xHCI root port (multi-device): each port
/// gets its own bring-up (port reset, controller slot, Address Device)
/// and registry slot, then a HID session where the device is a stick —
/// so several sticks coexist as /dev/js0, /dev/js1, ... (registry index
/// = js number). A port that fails costs its own resources only and
/// the loop moves on; the loop stops once the registry is full, before
/// a controller slot is wasted on a device that could not register.
///
/// # Safety
///
/// Requires a prior successful `usb::xhci::init`; boot-probe context
/// (single-threaded, SIE=0).
unsafe fn probe_xhci_ports() {
    unsafe {
        let owner = usb::dev::Owner::Xhci;
        for port in 0..usb::ports::owner_n_ports(owner) {
            let connected = usb::ports::owner_port_status(owner, port)
                .map(|st| st & PORT_CCS != 0)
                .unwrap_or(false);
            if !connected {
                continue;
            }
            if !usb::dev::has_free_slot() {
                crate::kwrn!(
                    "usb",
                    "device registry full, port %d skipped",
                    Arg::from(u32::from(port))
                );
                break;
            }
            probe_xhci_port(port);
        }
    }
}

/// Bring up, enumerate and (for a stick) attach the device on `port`.
/// Cleanup contract: a failure after the controller slot was granted
/// gives the slot back and frees its pages — inside `bringup_device`
/// for bring-up failures, here for a device the registry refused
/// (non-HID class, malformed descriptors, full registry).
///
/// # Safety
///
/// Requires a prior successful `usb::xhci::init`; boot-probe context.
unsafe fn probe_xhci_port(port: u8) {
    unsafe {
        let owner = usb::dev::Owner::Xhci;
        let slot_id = match usb::xhci::bringup_device(port) {
            Ok(id) => id,
            Err(e) => {
                crate::kwrn!(
                    "usb",
                    "xhci port %d bringup failed errno=%d",
                    Arg::from(u32::from(port)),
                    Arg::from(e.as_i64())
                );
                return;
            }
        };
        // SAFETY: bringup succeeded: controller up, `slot_id` addressed.
        let mut bus = usb::dev::requests::LiveBus::new(owner, usb::dev::requests::ADDR_DEFAULT)
            .with_hc_slot(slot_id);
        match usb::dev::probe_register(&mut bus, owner) {
            // SAFETY: boot-probe context per attach_hid's contract.
            Some(idx) => attach_hid(idx, &mut bus),
            // Not registered: nothing will ever poll this device, so
            // give its controller slot and pages back.
            None => usb::xhci::release_device(slot_id),
        }
    }
}

/// Enumerate the attached device through one platform controller
/// (EHCI/OHCI): a best-effort port reset, then enumeration into the
/// registry and the HID attach.
///
/// # Safety
///
/// Requires a prior successful `init_usb`; boot-probe context
/// (single-threaded, SIE=0).
unsafe fn probe_device(owner: usb::dev::Owner) {
    unsafe {
        // EHCI/OHCI init does not reset a port either: best-effort reset.
        reset_first_port(owner);
        // SAFETY: the controller init above succeeded, per LiveBus::new's contract.
        let mut bus = usb::dev::requests::LiveBus::new(owner, usb::dev::requests::ADDR_DEFAULT);
        // probe_register takes and releases USB_DEV_LOCK internally; the
        // returned index lets the HID attach continue on the same bus.
        if let Some(idx) = usb::dev::probe_register(&mut bus, owner) {
            // SAFETY: boot-probe context per attach_hid's contract; the
            // registry lock is free here.
            attach_hid(idx, &mut bus);
        }
    }
}

/// Reset + enable the first connected port of `owner`'s controller,
/// best-effort: a failure logs kwrn and enumeration proceeds anyway (it
/// reports its own failure if the port stays dead).
///
/// # Safety
///
/// Requires a prior successful `init_usb` with `owner`'s controller up.
unsafe fn reset_first_port(owner: usb::dev::Owner) {
    unsafe {
        if let Some(idx) = first_connected_port(owner)
            && let Err(e) = usb::ports::owner_port_reset(owner, idx)
                .and_then(|()| usb::ports::owner_port_enable(owner, idx))
        {
            crate::kwrn!(
                "usb",
                "port %d reset failed errno=%d",
                Arg::from(u32::from(idx)),
                Arg::from(e.as_i64())
            );
        }
    }
}

/// Attach a HID session to the freshly registered slot `idx` (ruling
/// 5): only HID-class interfaces get an attempt. The session attach
/// fetches the report descriptor, arms the interrupt-IN pipe through
/// the owner dispatch (xHCI endpoint context + posted buffers, OHCI
/// HCCA link), and installs the decode session. A NotStick (the
/// usb-kbd smoke case) logs kinf — the slot stays a plain enumerated
/// device; any other failure kwrns. Boot continues either way.
///
/// # Safety
///
/// Boot-probe context only — single-threaded, SIE=0, the owner's
/// controller initialized; `bus` is the LiveBus of the just-enumerated
/// device. `USB_DEV_LOCK` must not be held: attach's registry reads
/// take and release it per call, and its control transfers MMIO-wait.
unsafe fn attach_hid(idx: usize, bus: &mut dyn usb::dev::requests::ControlBus) {
    // Class gate (ruling 5): only HID interfaces get a session.
    // dev::slot takes the registry lock internally and releases it.
    let is_hid = usb::dev::slot(idx).map(|s| (s.class == usb::dev::desc::CLASS_HID, s.iface));
    let Some((true, iface)) = is_hid else {
        return;
    };
    // SAFETY: boot-probe context per the contract above; `bus` targets
    // the slot's device address after enumeration, and the registry
    // lock is not held.
    match unsafe { usb::hid::attach(idx, bus) } {
        Ok(()) => crate::kinf!(
            "usb",
            "hid stick attached slot=%d iface=%d",
            Arg::from(idx as u32),
            Arg::from(u32::from(iface))
        ),
        // NotStick (the usb-kbd case) — not smoke-greppable by design:
        // only the task-3 `dev vid=...` line is.
        Err(Errno::NoEnt) => crate::kinf!(
            "usb",
            "hid iface %d not a stick - enumerated only",
            Arg::from(u32::from(iface))
        ),
        // Other failures (bus error, malformed descriptor, arm failure
        // incl. the EHCI NoSys case): warn, boot continues.
        Err(e) => crate::kwrn!(
            "usb",
            "hid attach slot=%d failed errno=%d",
            Arg::from(idx as u32),
            Arg::from(e.as_i64())
        ),
    }
}
