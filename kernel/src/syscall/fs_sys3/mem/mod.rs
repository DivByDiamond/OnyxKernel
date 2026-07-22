mod brk;
mod mmap;
mod protect;

pub(super) use brk::sys_brk;
pub(super) use mmap::sys_mmap;
pub(super) use protect::{sys_mprotect, sys_munmap};
