//! Optional OC2R frame-publication companion. Pixel framebuffer ABI is unchanged.
use crate::libfdt::fdt::reader::{cstr_at, rd64};
use crate::libfdt::fdt::walk;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FramePresent {
    pub base: usize,
    pub framebuffer: usize,
}

/// # Safety
/// The FDT must be initialized and live throughout this boot-time walk.
pub unsafe fn find_frame_present(out: &mut [FramePresent]) -> usize {
    let mut count = 0;
    unsafe {
        walk(&mut |_name, props: &[(u32, &[u8])]| {
            if count == out.len() {
                return true;
            }
            let (mut compatible, mut enabled) = (false, true);
            let (mut base, mut size, mut framebuffer) = (0u64, 0u64, 0u64);
            for (name, data) in props {
                match cstr_at(*name) {
                    "compatible" => {
                        compatible = data
                            .split(|&b| b == 0)
                            .any(|s| s == b"oc2r,frame-present-v1")
                    }
                    "status" => enabled = *data == b"okay\0" || *data == b"ok\0",
                    // This extension uses two address and two size cells, and
                    // a fixed-width u64 target, regardless of node ordering.
                    "reg" if data.len() == 16 => {
                        base = rd64(data.as_ptr());
                        size = rd64(data.as_ptr().add(8));
                    }
                    "framebuffer-base" if data.len() == 8 => framebuffer = rd64(data.as_ptr()),
                    _ => {}
                }
            }
            if compatible
                && enabled
                && base != 0
                && base.is_multiple_of(4)
                && size >= 32
                && base.checked_add(size).is_some_and(|end| end <= 0x8000_0000)
                && framebuffer != 0
                && framebuffer < 0x8000_0000
                && !out[..count]
                    .iter()
                    .any(|old| old.base == base as usize || old.framebuffer == framebuffer as usize)
            {
                out[count] = FramePresent {
                    base: base as usize,
                    framebuffer: framebuffer as usize,
                };
                count += 1;
            }
            false
        });
    }
    count
}
