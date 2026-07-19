#![allow(non_camel_case_types)]

pub type u8 = core::primitive::u8;
pub type u16 = core::primitive::u16;
pub type u32 = core::primitive::u32;
pub type u64 = core::primitive::u64;
pub type usize = core::primitive::usize;
pub type i8 = core::primitive::i8;
pub type i16 = core::primitive::i16;
pub type i32 = core::primitive::i32;
pub type i64 = core::primitive::i64;
pub type isize = core::primitive::isize;
pub type paddr_t = u64;
pub type vaddr_t = u64;
pub type uptr = u64;

pub const KB: usize = 1024;
pub const MB: usize = 1024 * 1024;
pub const GB: usize = 1024 * 1024 * 1024;
pub const PAGE_SIZE: usize = 4096;
pub const PAGE_SHIFT: u32 = 12;
pub const PAGE_MASK: usize = !(PAGE_SIZE - 1);

#[inline]
pub const fn page_align_down(x: usize) -> usize {
    x & PAGE_MASK
}
#[inline]
pub const fn page_align_up(x: usize) -> usize {
    (x + PAGE_SIZE - 1) & PAGE_MASK
}
#[inline]
pub const fn is_aligned(x: usize) -> bool {
    (x & (PAGE_SIZE - 1)) == 0
}
#[inline]
pub const fn min(a: usize, b: usize) -> usize {
    if a < b {
        a
    } else {
        b
    }
}
#[inline]
pub const fn max(a: usize, b: usize) -> usize {
    if a > b {
        a
    } else {
        b
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_page_align_down() {
        assert_eq!(page_align_down(0), 0);
        assert_eq!(page_align_down(4095), 0);
        assert_eq!(page_align_down(4096), 4096);
        assert_eq!(page_align_down(4097), 4096);
        assert_eq!(page_align_down(8191), 4096);
        assert_eq!(page_align_down(8192), 8192);
    }
    #[test]
    fn test_page_align_up() {
        assert_eq!(page_align_up(0), 0);
        assert_eq!(page_align_up(1), 4096);
        assert_eq!(page_align_up(4096), 4096);
        assert_eq!(page_align_up(4097), 8192);
    }
    #[test]
    fn test_is_aligned() {
        assert!(is_aligned(0));
        assert!(is_aligned(4096));
        assert!(is_aligned(8192));
        assert!(!is_aligned(1));
        assert!(!is_aligned(4095));
    }
    #[test]
    fn test_min_max() {
        assert_eq!(min(10, 20), 10);
        assert_eq!(min(20, 10), 10);
        assert_eq!(max(10, 20), 20);
        assert_eq!(max(20, 10), 20);
        assert_eq!(min(42, 42), 42);
        assert_eq!(max(42, 42), 42);
    }
    #[test]
    fn test_constants() {
        assert_eq!(KB, 1024);
        assert_eq!(MB, 1024 * 1024);
        assert_eq!(GB, 1024 * 1024 * 1024);
        assert_eq!(PAGE_SIZE, 4096);
        assert_eq!(PAGE_SHIFT, 12);
        assert_eq!(PAGE_MASK, !(PAGE_SIZE - 1));
    }
}
