mod brk;
mod mmap;
mod protect;

pub use brk::sys_brk;
pub use mmap::sys_mmap;
pub use protect::{sys_mprotect, sys_munmap};
