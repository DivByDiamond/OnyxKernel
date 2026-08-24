//! Locates the OC2R/sedna monitor framebuffer in the device tree.
//!
//! The host mod exposes each monitor (and projector) as a `simple-framebuffer`
//! node under `/chosen`: MMIO `reg` address plus `width`/`height`/`stride`
//! properties and `format = "r5g6b5"`. Drawing into private RAM (the QEMU
//! fallback) is invisible to the host, so this node is the only usable display
//! on sedna.

use super::reader::{cstr_at, rd32, reg_base};
use super::walk;

pub struct SimpleFramebuffer {
    pub pa: usize,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
}

pub unsafe fn find_simple_framebuffer() -> Option<SimpleFramebuffer> {
    unsafe {
        let mut result: Option<SimpleFramebuffer> = None;
        walk(&mut |_name, props: &[(u32, &[u8])]| {
            let mut compatible = false;
            let mut base = 0u64;
            let mut width = 0u32;
            let mut height = 0u32;
            let mut stride = 0u32;
            for (name_off, data) in props {
                match cstr_at(*name_off) {
                    "compatible" => {
                        let mut start = 0;
                        while start < data.len() {
                            let end = data[start..]
                                .iter()
                                .position(|&b| b == 0)
                                .unwrap_or(data.len() - start);
                            if &data[start..start + end] == b"simple-framebuffer" {
                                compatible = true;
                            }
                            start += end + 1;
                        }
                    }
                    "reg" => base = reg_base(data),
                    "width" if data.len() >= 4 => width = rd32(data.as_ptr()),
                    "height" if data.len() >= 4 => height = rd32(data.as_ptr()),
                    "stride" if data.len() >= 4 => stride = rd32(data.as_ptr()),
                    _ => {}
                }
            }
            if compatible && base != 0 && base < 0x8000_0000 && width > 0 && height > 0 {
                // sedna maps MMIO devices below RAM (0x8000_0000); a reg inside RAM
                // would be a stale/foreign node — ignore it.
                result = Some(SimpleFramebuffer {
                    pa: base as usize,
                    width,
                    height,
                    stride,
                });
                return true;
            }
            false
        });
        result
    }
}
