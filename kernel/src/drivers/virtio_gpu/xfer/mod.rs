//! virtio-gpu control-queue transfer: queue setup (`queue`), the generic
//! synchronous command primitive (`send_cmd`), and the GPU protocol
//! commands built on top of it (`cmd`).
mod cmd;
mod queue;

pub use cmd::{cmd_attach, cmd_create2d, cmd_get_display_info, cmd_scanout};
pub use queue::setup_queue;

use crate::drivers::virtio::{VQ_DESC_F_NEXT, VqAvail, VqDesc, VqUsed};
use crate::mm::pmm;
use onyx_core::errno::KResult;

pub(super) const R_OK: u32 = 0x1100;
pub(super) const C_ATTACH: u32 = 0x106;
pub(super) const GPU_QSIZE: usize = 64;
pub(super) const C_GET_DISPLAY_INFO: u32 = 0x100;

/// # Safety
///
/// `cmd` must point to `len` bytes of device-accessible (physical/PMM) memory valid for the synchronous kick; queue pointers per `queue::kick`.
pub unsafe fn send_cmd(
    d: *mut VqDesc,
    a: *mut VqAvail,
    u: *mut VqUsed,
    lu: *mut u16,
    b: usize,
    cmd: *mut u8,
    len: u32,
) -> KResult<()> {
    // SAFETY: queue pointers per kick contract; slots masked % GPU_QSIZE; cmd is device-accessible for len bytes per the caller contract.
    unsafe {
        let rp = pmm::alloc_zero()? as usize;
        let i = (*a).idx as usize % GPU_QSIZE;
        *d.add(i) = VqDesc {
            addr: cmd as u64,
            len,
            flags: VQ_DESC_F_NEXT,
            next: ((i + 1) % GPU_QSIZE) as u16,
        };
        queue::kick(d, a, u, lu, b, rp)
    }
}
