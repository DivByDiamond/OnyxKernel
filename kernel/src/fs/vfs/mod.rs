pub mod chmod;
pub mod chown;
pub mod create;
pub mod dir;
pub mod dup;
pub mod file;
pub mod fsync;
pub mod mount;
pub mod ops;
pub mod rw;
pub mod seek;
pub mod symlink;
pub mod truncate;
pub mod unlink;
pub mod utimens;
pub mod vnode;

pub use chmod::*;
pub use chown::*;
pub use create::*;
pub use dir::*;
pub use dup::*;
pub use file::*;
pub use fsync::*;
pub use mount::*;
pub use ops::*;
pub use rw::*;
pub use seek::*;
pub use symlink::*;
pub use truncate::*;
pub use unlink::*;
pub use utimens::*;
pub use vnode::*;

pub(crate) use mount::{resolve_mount, G_MOUNTS, G_ROOT_FS};
pub(crate) use ops::{
    alloc_fd, fd_check, fd_check_perm, fd_clear, fd_get, fd_set, fd_set_cloexec, fd_update_pos,
    is_kernel_boot, G_KERNEL_FDS,
};

#[cfg(test)]
mod tests;
