//! USB device layer: descriptor parsing, control-request builders, the
//! enumeration sequence — and the boot device registry that the probe
//! fills and later consumers (HID polling, smoke checks) read.

pub mod desc;
pub mod enum_seq;
pub mod requests;

pub use enum_seq::enumerate;

use onyx_core::errno::{Errno, KResult};
use onyx_core::fmt::Arg;

use requests::ControlBus;

/// Which host controller owns an enumerated device.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Owner {
    Xhci,
    Ohci,
    Ehci,
}

/// Registry capacity: one slot per concurrently supported device (the
/// single-device boot enumeration fills one; the rest stay for the
/// multi-device follow-up).
pub const MAX_SLOTS: usize = 4;

/// One enumerated USB device as seen by registry consumers: the
/// interface chosen at enumeration, its interrupt-IN endpoint, and the
/// controller that owns the transfers.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct UsbSlot {
    pub active: bool,
    pub owner: Owner,
    /// Device address assigned during the enumeration pass.
    pub addr: u8,
    pub vendor: u16,
    pub product: u16,
    /// Interface class of the picked interface (HID for stick inputs).
    pub class: u8,
    /// Index of the picked interface within the parsed configuration.
    pub iface: u8,
    /// Interrupt-IN endpoint address for HID event polling.
    pub ep_in: u8,
    pub max_packet: u16,
    pub interval: u8,
}

/// Inactive-slot template; `owner` is never read while `active` is false.
const EMPTY: UsbSlot = UsbSlot {
    active: false,
    owner: Owner::Xhci,
    addr: 0,
    vendor: 0,
    product: 0,
    class: 0,
    iface: 0,
    ep_in: 0,
    max_packet: 0,
    interval: 0,
};

// Registry access is serialized on the device lock: the boot probe runs
// single-threaded with SIE=0 (crate::sync), and runtime consumers
// (HID polling) take the same lock.
static USB_DEV_LOCK: crate::sync::SpinLock = crate::sync::SpinLock::new();
static mut SLOTS: [UsbSlot; MAX_SLOTS] = [EMPTY; MAX_SLOTS];

/// Read slot `idx` (under `USB_DEV_LOCK`); `None` when `idx` is out of
/// range or the slot is inactive.
pub fn slot(idx: usize) -> Option<UsbSlot> {
    if idx >= MAX_SLOTS {
        return None;
    }
    USB_DEV_LOCK.lock();
    // SAFETY: USB_DEV_LOCK serializes all registry access.
    let s = unsafe {
        let slots = &raw const SLOTS;
        (*slots)[idx]
    };
    USB_DEV_LOCK.unlock();
    if s.active { Some(s) } else { None }
}

/// Store `s` into slot `idx` (under `USB_DEV_LOCK`). `Err(Errno::Range)`
/// for an out-of-range `idx`; `Err(Errno::Busy)` when overwriting one
/// active slot with another — deactivate the old slot first.
pub fn set_slot(idx: usize, s: UsbSlot) -> KResult<()> {
    if idx >= MAX_SLOTS {
        return Err(Errno::Range);
    }
    USB_DEV_LOCK.lock();
    // SAFETY: USB_DEV_LOCK serializes all registry access.
    let result = unsafe {
        let slots = &raw mut SLOTS;
        if (*slots)[idx].active && s.active {
            Err(Errno::Busy)
        } else {
            (*slots)[idx] = s;
            Ok(())
        }
    };
    USB_DEV_LOCK.unlock();
    result
}

/// Boot-probe entry: enumerate the device on `bus`, and on success
/// store the slot in the first free registry position, returning that
/// position. The whole sequence runs under `USB_DEV_LOCK` (ruling 6);
/// the lock is released before returning, so the caller may continue
/// with control transfers (HID attach) without re-taking it. Any
/// failure — or a full registry — logs a warning and returns `None`:
/// boot continues either way. Success logs the greppable boot line.
pub fn probe_register(bus: &mut dyn ControlBus, owner: Owner) -> Option<usize> {
    USB_DEV_LOCK.lock();
    let result = match enumerate(bus, owner) {
        Ok(s) => {
            // SAFETY: USB_DEV_LOCK is held for the whole function.
            let free = unsafe {
                let slots = &raw const SLOTS;
                (*slots).iter().position(|d| !d.active)
            };
            match free {
                Some(idx) => {
                    // SAFETY: USB_DEV_LOCK held; `idx` is a free slot.
                    unsafe {
                        let slots = &raw mut SLOTS;
                        (*slots)[idx] = s;
                    }
                    crate::kinf!(
                        "usb",
                        "dev vid=%x pid=%x class=%x",
                        Arg::from(s.vendor as u32),
                        Arg::from(s.product as u32),
                        Arg::from(s.class as u32)
                    );
                    Some(idx)
                }
                None => {
                    crate::kwrn!("usb", "device registry full");
                    None
                }
            }
        }
        Err(e) => {
            crate::kwrn!(
                "usb",
                "dev enumerate failed errno=%d",
                Arg::from(e.as_i64())
            );
            None
        }
    };
    USB_DEV_LOCK.unlock();
    result
}

#[cfg(test)]
mod tests;
