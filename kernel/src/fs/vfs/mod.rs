//! VFS: virtual filesystem layer.
//!
//! Subsystems:
//! - `fd`   — file-descriptor table and per-fd I/O (rw, seek, dup, fsync)
//! - `meta` — metadata operations (chmod, chown, truncate, utimens)
//! - `node` — namespace objects (create, unlink, symlink, dir, vnode core)
//! - `mount` — mount table and path resolution entry
//!
//! Every operation module is re-exported here so the historical
//! `vfs::<name>` paths keep working unchanged.
pub mod mount;

mod fd;
mod meta;
mod node;

#[cfg(test)]
mod tests;

pub use fd::{dup, file, fsync, ops, rw, seek};
pub use meta::{chmod, chown, truncate, utimens};
pub use node::{create, dir, symlink, unlink, vnode};

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

pub(crate) use mount::{G_ROOT_FS, resolve_mount};
pub(crate) use ops::{
    alloc_fd, fd_check, fd_check_perm, fd_clear, fd_get, fd_set, fd_set_cloexec, fd_set_flags,
    fd_update_pos, is_kernel_boot,
};
