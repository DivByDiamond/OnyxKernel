//! Opt-in OC2R completed-frame publication; absent companions keep legacy output.
//! Console transactions restore immediate mode after committing, so existing
//! native framebuffer applications retain their original write-through ABI.
use super::devices::MAX_FRAMEBUFFERS;
use core::sync::atomic::{AtomicUsize, Ordering, compiler_fence};

const MAGIC: u32 = 0x3150_464f;
const BEGIN: u32 = 1;
const COMMIT: u32 = 2;
const ABORT: u32 = 3;
const DISABLE: u32 = 4;
const ACTIVE: u32 = 1;
const NEGOTIATED: u32 = 2;
static CONTROLS: [AtomicUsize; MAX_FRAMEBUFFERS] =
    [const { AtomicUsize::new(0) }; MAX_FRAMEBUFFERS];

fn io_fence() {
    compiler_fence(Ordering::SeqCst);
    #[cfg(any(target_arch = "riscv32", target_arch = "riscv64"))]
    // Device memory and framebuffer stores must precede COMMIT, including on SMP.
    unsafe {
        core::arch::asm!("fence iorw, iorw", options(nostack));
    }
    compiler_fence(Ordering::SeqCst);
}

#[derive(Clone, Copy)]
struct Controller(usize);
impl Controller {
    // SAFETY: caller has validated a live aligned companion MMIO extent >=32.
    unsafe fn read(self, offset: usize) -> u32 {
        unsafe { core::ptr::read_volatile((self.0 + offset) as *const u32) }
    }
    fn command(self, value: u32) {
        io_fence();
        // SAFETY: Controllers are constructed only by validated boot-time probe.
        unsafe {
            core::ptr::write_volatile((self.0 + 8) as *mut u32, value);
        }
        io_fence();
    }
    unsafe fn probe(base: usize) -> Option<Self> {
        let c = Self(base);
        unsafe { (c.read(0) == MAGIC && c.read(4) == 1 && c.read(16) & 1 != 0).then_some(c) }
    }
}

/// # Safety
/// Boot-time only: initialized DTB, registered framebuffers and identity-mapped
/// MMIO. Only companions explicitly linked to a known framebuffer are probed.
pub unsafe fn init_from_fdt() {
    let mut nodes = [crate::libfdt::fdt::FramePresent::default(); MAX_FRAMEBUFFERS];
    let count = unsafe { crate::libfdt::fdt::find_frame_present(&mut nodes) };
    for node in &nodes[..count] {
        for (index, slot) in CONTROLS.iter().enumerate().take(super::count()) {
            if super::display(index).is_some_and(|fb| fb.base == node.framebuffer) {
                if let Some(c) = unsafe { Controller::probe(node.base) } {
                    // Recover abandoned transactions after a guest warm reboot.
                    c.command(ABORT);
                    c.command(DISABLE);
                    slot.store(c.0, Ordering::Release);
                    crate::kinf!(
                        "fb",
                        "frame publication companion at %p",
                        onyx_core::fmt::Arg::from(c.0)
                    );
                }
                break;
            }
        }
    }
}

fn console_controller() -> Option<Controller> {
    let base = super::fb_base_pa();
    CONTROLS.iter().enumerate().find_map(|(index, slot)| {
        if super::display(index).is_some_and(|fb| fb.base == base) {
            let control = slot.load(Ordering::Acquire);
            if control != 0 {
                return Some(Controller(control));
            }
        }
        None
    })
}

/// Cancel a console transaction before replacing its framebuffer geometry.
/// Called under the framebuffer init single-threaded contract.
pub(super) fn reset_console() {
    if let Some(c) = console_controller() {
        c.command(ABORT);
        c.command(DISABLE);
    }
}

/// Bounded transaction guard. A busy companion defers console publication;
/// callers must retain their pending dirty rectangle and retry later.
pub struct Publication {
    controller: Option<Controller>,
}
impl Publication {
    pub fn begin_console() -> Option<Self> {
        let controller = console_controller();
        if let Some(c) = controller {
            // Do not interfere with a future native graphics owner's transaction.
            if unsafe { c.read(12) } != 0 {
                return None;
            }
            c.command(BEGIN);
            if unsafe { c.read(12) } != (ACTIVE | NEGOTIATED) {
                c.command(ABORT);
                c.command(DISABLE);
                return None;
            }
        }
        Some(Self { controller })
    }
    pub fn commit(mut self) -> bool {
        let Some(c) = self.controller else {
            return true;
        };
        c.command(COMMIT);
        let committed = unsafe { c.read(12) } == NEGOTIATED;
        if committed {
            c.command(DISABLE);
            self.controller = None;
        }
        committed
    }
}
impl Drop for Publication {
    fn drop(&mut self) {
        if let Some(c) = self.controller {
            c.command(ABORT);
            c.command(DISABLE);
        }
    }
}

#[cfg(test)]
mod tests;
