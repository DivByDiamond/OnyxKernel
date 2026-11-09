//! Device-tree node finders, grouped by the subsystem they expose.

mod display;
mod system;
mod sdhci;
mod uart;
mod virtio;

pub use display::{
    FramePresent, SimpleFramebuffer, find_frame_present, find_simple_framebuffer,
    find_simple_framebuffers, is_qemu, is_sedna, model,
};
pub use sdhci::find_sdhci;
pub use system::{find_clint, find_plic, init, memory};
#[cfg(test)]
pub(crate) use system::init_from;
pub use uart::find_uart;
pub use virtio::find_virtio;
