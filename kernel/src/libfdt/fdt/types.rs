use onyx_core::parser::be32;

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
