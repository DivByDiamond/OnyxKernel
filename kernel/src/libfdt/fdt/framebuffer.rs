//! Locates the OC2R/sedna monitor framebuffer in the device tree.
//!
//! The host mod exposes each monitor (and projector) as a `simple-framebuffer`
//! node under `/soc` (or legacy `/chosen`): MMIO `reg` address plus `width`/`height`/`stride`
//! properties and `format = "r5g6b5"`. Drawing into private RAM (the QEMU
//! fallback) is invisible to the host, so this node is the only usable display
//! on sedna.

use super::reader::{cstr_at, rd32, reg_base};
use super::walk;

#[derive(Clone, Copy, Default)]
pub struct SimpleFramebuffer {
    pub pa: usize,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub bpp: u32,
}

/// # Safety
///
/// `fdt::init()` must have succeeded; called once during single-threaded
/// boot (before secondary harts start).
pub unsafe fn find_simple_framebuffer() -> Option<SimpleFramebuffer> {
    let mut first = [SimpleFramebuffer::default(); 1];
    if unsafe { find_simple_framebuffers(&mut first) } == 1 { Some(first[0]) } else { None }
}

/// Enumerates valid enabled displays in DT order, independently of parent path.
/// # Safety
/// Same boot-time DTB lifetime contract as `find_simple_framebuffer`.
pub unsafe fn find_simple_framebuffers(out: &mut [SimpleFramebuffer]) -> usize {
    unsafe {
        let mut count = 0;
        walk(&mut |_name, props: &[(u32, &[u8])]| {
            if count == out.len() { return true; }
            let mut compatible = false;
            let mut enabled = true;
            let mut bpp = 0;
            let mut extent = 0u64;
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
                    "reg" => {
                        base = reg_base(data);
                        extent = if data.len() >= 16 {
                            ((rd32(data.as_ptr().add(8)) as u64) << 32) | rd32(data.as_ptr().add(12)) as u64
                        } else if data.len() >= 12 { rd32(data.as_ptr().add(8)) as u64
                        } else if data.len() >= 8 { rd32(data.as_ptr().add(4)) as u64 } else { 0 };
                    }
                    "status" => enabled = data == b"okay\0" || data == b"ok\0",
                    "format" => bpp = if data == b"r5g6b5\0" { 16 } else if data == b"x8r8g8b8\0" { 32 } else { 0 },
                    "width" if data.len() >= 4 => width = rd32(data.as_ptr()),
                    "height" if data.len() >= 4 => height = rd32(data.as_ptr()),
                    "stride" if data.len() >= 4 => stride = rd32(data.as_ptr()),
                    _ => {}
                }
            }
            let bytes = bpp / 8;
            let row = width.checked_mul(bytes);
            let pitch = if stride == 0 { row.unwrap_or(0) } else { stride };
            let size = (pitch as u64).checked_mul(height as u64);
            if compatible && enabled && bytes > 0 && base != 0 && base < 0x8000_0000
                && width > 0 && height > 0 && row.is_some_and(|r| pitch >= r)
                && size.is_some_and(|s| s <= extent && base.checked_add(s).is_some_and(|end| end <= 0x8000_0000))
                && !out[..count].iter().any(|old| old.pa == base as usize) {
                out[count] = SimpleFramebuffer { pa: base as usize, width, height, stride: pitch, bpp };
                count += 1;
            }
            false
        });
        count
    }
}
