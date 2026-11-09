//! virtio-input MMIO driver — root module.
//!
//! Probe/init + event decode. Reads raw input events (keyboard, mouse,
//! tablet) from the device's event virtqueue and translates them to
//! the unified `drivers::input::Event` representation.

// `&raw mut` on a `static mut` is deliberate: clippy's suggested bare `STATIC` would
// create a reference to it and trip `static_mut_refs`.
#![allow(clippy::deref_addrof)]
use crate::drivers::virtio::{
    R_DEVICE_ID, R_MAGIC_VALUE, R_STATUS, VIRTQ_SIZE, VqAvail, VqDesc, VqUsed, reg_r, reg_w,
};
use crate::mm::pmm;
use core::ptr;
use onyx_core::errno::{Errno, KResult};

pub const VIRTIO_ID_INPUT: u32 = 18;
// Pre-post the full negotiated queue. Sedna drains a staged event batch before
// running the guest CPU; posting only eight buffers loses bursts after two keys.
pub const N_EVENTS: usize = VIRTQ_SIZE;

/// virtio-input event (spec §5.5.5).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VirtioInputEvent {
    pub type_: u16,
    pub code: u16,
    pub value: u32,
}

#[derive(Clone, Copy)]
pub(crate) struct InDev {
    pub ready: bool,
    pub base: usize,
    pub modern: bool,
    pub desc: *mut VqDesc,
    pub avail: *mut VqAvail,
    pub used: *mut VqUsed,
    pub last_used: u16,
    pub ev_buf: *mut VirtioInputEvent,
    pub _head: usize,
}

const EMPTY: InDev = InDev {
    ready: false,
    base: 0,
    modern: false,
    desc: ptr::null_mut(),
    avail: ptr::null_mut(),
    used: ptr::null_mut(),
    last_used: 0,
    ev_buf: ptr::null_mut(),
    _head: 0,
};

const _: () = assert!(N_EVENTS * core::mem::size_of::<VirtioInputEvent>() <= pmm::PAGE_SIZE);
pub const MAX_INPUT_DEVICES: usize = 32;

struct InputDevices {
    devices: [InDev; MAX_INPUT_DEVICES],
    next: usize,
    held: [[u64; 12]; MAX_INPUT_DEVICES],
    counts: [u8; 768],
}
impl InputDevices {
    const fn new() -> Self {
        Self {
            devices: [EMPTY; MAX_INPUT_DEVICES],
            next: 0,
            held: [[0; 12]; MAX_INPUT_DEVICES],
            counts: [0; 768],
        }
    }
    fn slot(&self, base: usize) -> KResult<usize> {
        if self.devices.iter().any(|d| d.base == base && base != 0) {
            return Err(Errno::Busy);
        }
        self.devices
            .iter()
            .position(|d| d.base == 0)
            .ok_or(Errno::NoSpace)
    }
    /// Round-robin consumption prevents a busy pointer starving another keyboard.
    unsafe fn poll(&mut self) -> Option<decode::EventType> {
        for n in 0..MAX_INPUT_DEVICES {
            let slot = (self.next + n) % MAX_INPUT_DEVICES;
            let Some(event) = (unsafe { decode::poll_device(&mut self.devices[slot]) }) else {
                continue;
            };
            self.next = (slot + 1) % MAX_INPUT_DEVICES;
            return Some(self.aggregate(slot, event));
        }
        None
    }
    fn aggregate(&mut self, slot: usize, event: VirtioInputEvent) -> decode::EventType {
        if event.type_ == decode::EV_KEY {
            let key = event.code as usize;
            if key >= self.counts.len() || event.value > 2 {
                return decode::EventType::Syn;
            }
            let bit = 1u64 << (key % 64);
            let was_down = self.held[slot][key / 64] & bit != 0;
            if event.value == 0 {
                if !was_down {
                    return decode::EventType::Syn;
                }
                self.held[slot][key / 64] &= !bit;
                self.counts[key] -= 1;
                if self.counts[key] != 0 {
                    return decode::EventType::Syn;
                }
            } else if !was_down {
                if event.value == 2 {
                    return decode::EventType::Syn;
                }
                self.held[slot][key / 64] |= bit;
                self.counts[key] += 1;
            } else if event.value != 2 || matches!(event.code, 58 | 69 | 70) {
                return decode::EventType::Syn;
            }
        }
        decode::translate(event)
    }
}

// All register/ring/aggregate-state accesses are serialized across polling harts.
// Init runs once during boot; polling occurs with interrupts disabled (sync.rs).
static INPUT_LOCK: crate::sync::SpinLock = crate::sync::SpinLock::new();
static mut INPUTS: InputDevices = InputDevices::new();

/// # Safety
///
/// `base` must be a candidate virtio-mmio base from the FDT probe or the
/// QEMU virt fallback constants (identity-mapped at boot).
pub unsafe fn probe(base: usize) -> bool {
    // SAFETY: base is a candidate virtio-mmio base from the boot-time probe; reg_r reads only spec offsets (magic, device ID).
    unsafe {
        if reg_r(base, R_MAGIC_VALUE) != 0x7472_6976 {
            return false;
        }
        reg_r(base, R_DEVICE_ID) == VIRTIO_ID_INPUT
    }
}

/// # Safety
///
/// `base` must be a probed virtio-input MMIO base; must be called during
/// the single-threaded boot-time device probe, once per base.
pub unsafe fn init(base: usize) -> KResult<()> {
    INPUT_LOCK.lock();
    // SAFETY: the input lock excludes all registry/ring readers and writers.
    let result = unsafe {
        let inputs = &mut *(&raw mut INPUTS);
        match inputs.slot(base) {
            Ok(slot) => {
                let mut dev = EMPTY;
                let result = queue::init_device(base, &mut dev);
                if result.is_ok() {
                    inputs.devices[slot] = dev;
                } else {
                    // Stop DMA before releasing partial allocations. A failed
                    // device never occupies a registry slot or blocks a later one.
                    reg_w(base, R_STATUS, 0);
                    for pa in [
                        dev.desc as u64,
                        dev.avail as u64,
                        dev.used as u64,
                        dev.ev_buf as u64,
                    ] {
                        if pa != 0 {
                            pmm::free(pa);
                        }
                    }
                }
                result
            }
            Err(error) => Err(error),
        }
    };
    INPUT_LOCK.unlock();
    result
}

/// Poll every initialized device fairly, without touching incomplete queues.
pub fn poll() -> Option<decode::EventType> {
    INPUT_LOCK.lock();
    // SAFETY: lock serializes all queue consumers, including cross-hart syscalls.
    let event = unsafe { (&mut *(&raw mut INPUTS)).poll() };
    INPUT_LOCK.unlock();
    event
}

mod queue;
use queue::push;

pub mod decode;
pub use decode::{EventType, event_type, poll_unified};

#[cfg(test)]
mod tests;
