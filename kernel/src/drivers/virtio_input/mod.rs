//! virtio-input MMIO driver — root module.
//!
//! Probe/init + event decode. Reads raw input events (keyboard, mouse,
//! tablet) from the device's event virtqueue and translates them to
//! the unified `drivers::input::Event` representation.
use crate::drivers::virtio::{
    R_DEVICE_ID, R_GUEST_FEATURES, R_GUEST_FEATURES_SEL, R_HOST_FEATURES, R_HOST_FEATURES_SEL,
    R_MAGIC_VALUE, R_QUEUE_AVAIL_HIGH, R_QUEUE_AVAIL_LOW, R_QUEUE_DESC_HIGH, R_QUEUE_DESC_LOW,
    R_QUEUE_NUM, R_QUEUE_READY, R_QUEUE_SEL, R_QUEUE_USED_HIGH, R_QUEUE_USED_LOW, R_STATUS,
    R_VERSION, VIRTIO_F_VERSION_1, VIRTIO_S_ACK, VIRTIO_S_DRIVER, VIRTIO_S_DRIVER_OK,
    VIRTIO_S_FEATURES_OK, VIRTQ_SIZE, VQ_DESC_F_WRITE, VqAvail, VqDesc, VqUsed, reg_r, reg_w,
};
use crate::mm::pmm;
use core::ptr;
use onyx_core::errno::{Errno, KResult};

pub const VIRTIO_ID_INPUT: u32 = 18;
pub const N_EVENTS: usize = 8;

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
                let result = init_device(base, &mut dev);
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

unsafe fn init_device(base: usize, dev: &mut InDev) -> KResult<()> {
    unsafe {
        let version = reg_r(base, R_VERSION);
        let modern = version >= 2;
        dev.base = base;
        dev.modern = modern;
        reg_w(base, R_STATUS, 0);
        reg_w(base, R_STATUS, VIRTIO_S_ACK | VIRTIO_S_DRIVER);
        reg_w(base, R_HOST_FEATURES_SEL, 0);
        let hf = reg_r(base, R_HOST_FEATURES);
        reg_w(base, R_GUEST_FEATURES_SEL, 0);
        reg_w(base, R_GUEST_FEATURES, hf & 0x1FFF_FFFF);
        if modern {
            reg_w(base, R_HOST_FEATURES_SEL, 1);
            let high = reg_r(base, R_HOST_FEATURES);
            reg_w(base, R_HOST_FEATURES_SEL, 0);
            if high & VIRTIO_F_VERSION_1 == 0 {
                return Err(Errno::Inval);
            }
            reg_w(base, R_GUEST_FEATURES_SEL, 1);
            reg_w(base, R_GUEST_FEATURES, VIRTIO_F_VERSION_1);
            reg_w(base, R_GUEST_FEATURES_SEL, 0);
        }
        if modern {
            reg_w(
                base,
                R_STATUS,
                VIRTIO_S_ACK | VIRTIO_S_DRIVER | VIRTIO_S_FEATURES_OK,
            );
            if reg_r(base, R_STATUS) & VIRTIO_S_FEATURES_OK == 0 {
                return Err(Errno::Inval);
            }
        }
        setup_event_queue(dev)?;
        reg_w(
            base,
            R_STATUS,
            VIRTIO_S_ACK
                | VIRTIO_S_DRIVER
                | VIRTIO_S_DRIVER_OK
                | if modern { VIRTIO_S_FEATURES_OK } else { 0 },
        );
        dev.ready = true;
        Ok(())
    }
}

/// # Safety
///
/// Must be called from `init` with dev.base set to a probed base, during
/// single-threaded boot-time probe.
unsafe fn setup_event_queue(dev: &mut InDev) -> KResult<()> {
    // SAFETY: called from init on a probed base during single-threaded boot; rings and the N_EVENTS-entry buffer are fresh contiguous PMM pages stored into G_IN and registered before use; desc slots 0..N_EVENTS pre-posted, N_EVENTS < VIRTQ_SIZE.
    unsafe {
        let base = dev.base;
        reg_w(base, R_QUEUE_SEL, 0);
        reg_w(base, R_QUEUE_NUM, VIRTQ_SIZE as u32);
        dev.desc = pmm::alloc_zero()? as *mut VqDesc;
        dev.avail = pmm::alloc_zero()? as *mut VqAvail;
        dev.used = pmm::alloc_zero()? as *mut VqUsed;
        dev.ev_buf = pmm::alloc_zero()? as *mut VirtioInputEvent;
        let desc_pa = dev.desc as usize;
        let avail_pa = dev.avail as usize;
        let used_pa = dev.used as usize;
        let ev_pa = dev.ev_buf;
        reg_w(base, R_QUEUE_DESC_LOW, desc_pa as u32);
        reg_w(base, R_QUEUE_DESC_HIGH, ((desc_pa as u64) >> 32) as u32);
        reg_w(base, R_QUEUE_AVAIL_LOW, avail_pa as u32);
        reg_w(base, R_QUEUE_AVAIL_HIGH, ((avail_pa as u64) >> 32) as u32);
        reg_w(base, R_QUEUE_USED_LOW, used_pa as u32);
        reg_w(base, R_QUEUE_USED_HIGH, ((used_pa as u64) >> 32) as u32);
        reg_w(base, R_QUEUE_READY, 1);
        // Pre-post N_EVENTS descriptors for incoming events.
        for i in 0..N_EVENTS {
            let off = (ev_pa as usize) + i * core::mem::size_of::<VirtioInputEvent>();
            (*dev.desc.add(i)) = VqDesc {
                addr: off as u64,
                len: core::mem::size_of::<VirtioInputEvent>() as u32,
                flags: VQ_DESC_F_WRITE,
                next: 0,
            };
            push(dev, i);
        }
        Ok(())
    }
}

/// # Safety
///
/// dev.avail must have been initialized by `init` (non-null); `idx` must
/// be a valid descriptor index (< N_EVENTS).
unsafe fn push(dev: &mut InDev, idx: usize) {
    // SAFETY: dev.avail is a PMM ring set up by init; ring slot masked % VIRTQ_SIZE per spec; volatile write + SeqCst fence order entry before idx bump.
    unsafe {
        let i = ptr::read_volatile(ptr::addr_of!((*dev.avail).idx));
        ptr::write_volatile(
            ptr::addr_of_mut!((*dev.avail).ring[(i as usize) % VIRTQ_SIZE]),
            idx as u16,
        );
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        ptr::write_volatile(ptr::addr_of_mut!((*dev.avail).idx), i.wrapping_add(1));
    }
}

pub mod decode;
pub use decode::{EventType, event_type, poll_unified};

#[cfg(test)]
mod tests;
