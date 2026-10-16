//! Boot-time framebuffer registry. Index zero remains the console display.
use onyx_core::errno::{Errno, KResult};

pub const MAX_FRAMEBUFFERS: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Framebuffer {
    pub base: usize,
    pub width: usize,
    pub height: usize,
    pub pitch: usize,
    pub bpp: usize,
    pub size: usize,
}

impl Framebuffer {
    pub fn new(base: usize, width: usize, height: usize, pitch: usize, bpp: usize) -> KResult<Self> {
        let bytes = match bpp { 16 => 2, 24 => 3, 32 => 4, _ => return Err(Errno::Inval) };
        let row = width.checked_mul(bytes).ok_or(Errno::Overflow)?;
        let size = pitch.checked_mul(height).ok_or(Errno::Overflow)?;
        if base == 0 || width == 0 || height == 0 || pitch < row || size > u32::MAX as usize
            || ((bpp == 16 || bpp == 32) && (base % bytes != 0 || pitch % bytes != 0)) {
            return Err(Errno::Inval);
        }
        base.checked_add(size).ok_or(Errno::Overflow)?;
        Ok(Self { base, width, height, pitch, bpp, size })
    }
}

// Written only during single-hart display initialization, before userspace/SMP.
static mut DISPLAYS: [Option<Framebuffer>; MAX_FRAMEBUFFERS] = [None; MAX_FRAMEBUFFERS];
static mut COUNT: usize = 0;

/// # Safety
/// Single-threaded boot only; the descriptor must refer to live identity-mapped display memory.
pub unsafe fn register(display: Framebuffer) -> KResult<usize> {
    unsafe {
        for i in 0..COUNT {
            if DISPLAYS[i].is_some_and(|old| old.base == display.base) {
                return if DISPLAYS[i] == Some(display) { Ok(i) } else { Err(Errno::Inval) };
            }
        }
        if COUNT == MAX_FRAMEBUFFERS { return Err(Errno::Inval); }
        let index = COUNT;
        DISPLAYS[index] = Some(display);
        COUNT += 1;
        Ok(index)
    }
}

pub fn count() -> usize { unsafe { COUNT } }
pub fn get(index: usize) -> Option<Framebuffer> {
    unsafe { if index < COUNT { DISPLAYS[index] } else { None } }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_surface_bounds_before_exposing_memory() {
        assert_eq!(Framebuffer::new(0x30000000, 640, 400, 1280, 16).unwrap().size, 512000);
        assert!(Framebuffer::new(0, 640, 400, 1280, 16).is_err());
        assert!(Framebuffer::new(0x30000000, 640, 400, 1279, 16).is_err());
        assert!(Framebuffer::new(0x30000001, 640, 400, 1280, 16).is_err());
        assert!(Framebuffer::new(0x30000000, usize::MAX, 2, 1280, 32).is_err());
        assert!(Framebuffer::new(usize::MAX - 3, 2, 2, 4, 16).is_err());
    }
}
