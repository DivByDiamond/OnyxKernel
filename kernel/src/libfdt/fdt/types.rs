pub struct FdtMemory {
    pub base: u64,
    pub size: u64,
}

#[derive(Clone, Copy)]
pub struct FdtMmio {
    pub base: u64,
    pub irq: u32,
    pub reg_shift: u32,
}

pub(crate) const FDT_MAGIC: u32 = 0xD00D_FEED;
pub(crate) const FDT_BEGIN_NODE: u32 = 0x1;
pub(crate) const FDT_END_NODE: u32 = 0x2;
pub(crate) const FDT_PROP: u32 = 0x3;
pub(crate) const FDT_NOP: u32 = 0x4;
pub(crate) const FDT_END: u32 = 0x9;

pub(crate) static mut G_DTB: usize = 0;
pub(crate) static mut G_STRUCT: usize = 0;
pub(crate) static mut G_STRINGS: usize = 0;
pub(crate) static mut G_STRUCT_SIZE: usize = 0;
/// Bounds-checking fix (todo P1 #7): totalsize from the FDT header (offset
/// 4). Set by init_from once the magic validates; 0 before. Every global
/// offset/size below is validated against it so a corrupt header cannot
/// point the walker outside the blob.
pub(crate) static mut G_TOTALSIZE: usize = 0;
/// Bounds-checking fix (todo P1 #7): size_dt_strings (header offset 0x20).
/// Bounds the string-block scans (cstr_at / prop_name).
pub(crate) static mut G_STRINGS_SIZE: usize = 0;
