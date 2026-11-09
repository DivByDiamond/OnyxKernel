//! Host integration tests for the ANSI console pipeline.
//!
//! Split by responsibility:
//! - [`pipeline`]: double-buffered paint pipeline and direct-mode fallback;
//! - [`present`]: present-time cursor stamping, erase and region copies;
//! - [`scale`]: scaled console geometry and dirty-glyph publishing.
use crate::drivers::fb;

mod pipeline;
mod present;
mod scale;

/// Serializes tests that mutate the global fb/ANSI state.
static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

const W: usize = 160;
const H: usize = 64;
const STRIDE: usize = W * 4;

struct Surfaces {
    front: Vec<u8>,
    back: Vec<u8>,
}

impl Drop for Surfaces {
    fn drop(&mut self) {
        fb::test_clear_back();
        fb::test_uninstall_front();
    }
}

fn install() -> Surfaces {
    // Sentinel pattern so any front-buffer write is detectable.
    let mut front = vec![0u8; STRIDE * H];
    for (i, b) in front.iter_mut().enumerate() {
        *b = (i % 251) as u8;
    }
    let back = vec![0u8; STRIDE * H];
    // SAFETY: test-only front installation; the surface outlives the test.
    unsafe {
        fb::test_install_front(front.as_mut_ptr(), W, H, STRIDE, 32);
    }
    Surfaces { front, back }
}

fn back_ptr(s: &Surfaces) -> usize {
    s.back.as_ptr() as usize
}
