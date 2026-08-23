use super::G_FB;
use crate::font;

const WORD: usize = core::mem::size_of::<usize>();

/// Volatile word-sized copy of `len` bytes from `src` to `dst`.
/// Regions must not overlap backwards by less than one row (caller guarantees
/// dst < src, so forward copying is safe). Uses `usize` words when both
/// pointers are word-aligned and share alignment; otherwise falls back to
/// byte-wise volatile copies. No `core::ptr::copy`: the framebuffer is MMIO.
///
/// # Safety
/// `[src, src+len)` and `[dst, dst+len)` must be mapped framebuffer memory,
/// `dst <= src`, and the ranges must not overlap in a way that forward
/// byte/word copies would corrupt (dst ahead of src is forbidden).
pub(super) unsafe fn vcopy(dst: *mut u8, src: *const u8, len: usize) {
    // SAFETY: caller guarantees both regions are valid, mapped, and disjoint
    // enough for a forward copy; all accesses are volatile MMIO-safe.
    unsafe {
        if !(dst as usize).is_multiple_of(WORD) || ((dst as usize) ^ (src as usize)) & (WORD - 1) != 0 {
            for i in 0..len {
                let b = core::ptr::read_volatile(src.add(i));
                core::ptr::write_volatile(dst.add(i), b);
            }
            return;
        }
        let mut i = 0;
        while i + WORD <= len {
            let w = core::ptr::read_volatile(src.add(i) as *const usize);
            core::ptr::write_volatile(dst.add(i) as *mut usize, w);
            i += WORD;
        }
        while i < len {
            let b = core::ptr::read_volatile(src.add(i));
            core::ptr::write_volatile(dst.add(i), b);
            i += 1;
        }
    }
}

/// Volatile word-sized zero fill of `len` bytes at `dst`, with the same
/// alignment strategy as [`vcopy`].
///
/// # Safety
/// `[dst, dst+len)` must be mapped framebuffer memory.
pub(super) unsafe fn vzero(dst: *mut u8, len: usize) {
    // SAFETY: caller guarantees the region is valid mapped framebuffer
    // memory; every store below is volatile.
    unsafe {
        if !(dst as usize).is_multiple_of(WORD) {
            for i in 0..len {
                core::ptr::write_volatile(dst.add(i), 0);
            }
            return;
        }
        let mut i = 0;
        while i + WORD <= len {
            core::ptr::write_volatile(dst.add(i) as *mut usize, 0);
            i += WORD;
        }
        while i < len {
            core::ptr::write_volatile(dst.add(i), 0);
            i += 1;
        }
    }
}

pub fn scroll() {
    // SAFETY: G_FB describes the active MMIO framebuffer; base..base+total is
    // mapped when enabled, and vcopy/vzero stay inside [0, total).
    unsafe {
        if !G_FB.enabled {
            return;
        }
        let base = G_FB.base;
        let row_bytes = font::FONT_H * G_FB.pitch;
        let total = G_FB.height * G_FB.pitch;
        vcopy(base, base.add(row_bytes), total - row_bytes);
        vzero(base.add(total - row_bytes), row_bytes);
    }
}

pub fn flush() {}
