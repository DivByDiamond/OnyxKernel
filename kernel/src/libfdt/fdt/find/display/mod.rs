//! Display-related nodes: the simple framebuffer, the frame-publication
//! companion, and platform/model detection.

mod frame_present;
mod framebuffer;
mod model;

pub use frame_present::{FramePresent, find_frame_present};
pub use framebuffer::{SimpleFramebuffer, find_simple_framebuffer, find_simple_framebuffers};
pub use model::{is_qemu, is_sedna, model};
