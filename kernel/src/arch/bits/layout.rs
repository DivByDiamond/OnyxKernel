pub const USER_BASE: u64 = 0x10000;

#[cfg(target_pointer_width = "64")]
pub const USER_TOP: u64 = 0x4000_0000;
#[cfg(target_pointer_width = "32")]
pub const USER_TOP: u64 = 0x8000_0000;

pub const KERNEL_BASE: u64 = 0x8020_0000;

pub const USER_HEAP_BASE: u64 = 0x0100_0000;
pub const USER_HEAP_SIZE: u64 = 64 * 1024 * 1024;
pub const USER_STACK_TOP: u64 = USER_TOP - 4096;
pub const USER_STACK_PAGES: usize = 64;
pub const USER_HEAP_PAGES: usize = 16;
