use super::super::state::FONT_H;
use core::sync::atomic::Ordering;
use crate::drivers::fb;

fn fb_info() -> (usize, usize, usize, usize, usize) {
    fb::info()
}
pub(super) fn paint_info() -> (usize, usize, usize, usize) {
    let (_, pitch, bpp, height, _) = fb_info();
    (paint_base_addr(), pitch, bpp, height)
}
pub(super) fn paint_base_addr() -> usize {
    fb::paint_base() as usize
}

pub(super) fn surface() -> Option<(usize, usize)> {
    let (base, pitch, bpp, height) = paint_info();
    if base == 0 || pitch == 0 || (bpp != 32 && bpp != 16) {
        return None;
    }
    Some((base, pitch * (height / FONT_H) * FONT_H))
}

pub(super) fn ensure_saved(bytes: usize) -> usize {
    let pages = bytes.div_ceil(crate::mm::pmm::PAGE_SIZE);
    match unsafe { crate::mm::pmm::alloc_n(pages) } {
        Ok(pa) => {
            match super::G_SAVE_PA.compare_exchange(0, pa as usize, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => pa as usize,
                Err(existing) => {
                    unsafe { crate::mm::pmm::free(pa) };
                    existing
                }
            }
        }
        Err(_) => 0,
    }
}

pub(super) fn clear_surface(base: usize, bytes: usize, color: u32) {
    let (_, _, bpp, _) = paint_info();
    clear_row_bytes(base, bytes, color, bpp);
}

pub(super) fn clear_row_bytes(dst: usize, len: usize, color: u32, bpp: usize) {
    // SAFETY: callers pass dst = base + row*pitch*FONT_H inside validated fb; len = pitch*FONT_H.
    unsafe {
        if bpp == 16 {
            let px = fb::rgb32_to_r5g6b5(color).to_le();
            let d = dst as *mut u16;
            let words = len / 2;
            for i in 0..words {
                *d.add(i) = px;
            }
            if len & 1 != 0 {
                *(dst as *mut u8).add(len - 1) = (px & 0xFF) as u8;
            }
        } else {
            let bytes = color.to_le_bytes();
            let d = dst as *mut u8;
            for i in 0..len {
                *d.add(i) = bytes[i & 3];
            }
        }
    }
}
