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

pub(crate) use fd::{dup, file, fsync, ops, rw, seek, table};
pub(crate) use meta::{chmod, chown, truncate, utimens};
pub(crate) use node::{create, dir, symlink, unlink, vnode};

pub(crate) use chmod::*;
pub(crate) use chown::*;
pub(crate) use create::*;
pub(crate) use dir::*;
pub(crate) use dup::*;
pub(crate) use file::*;
pub(crate) use fsync::*;
pub(crate) use mount::*;
pub(crate) use ops::*;
pub(crate) use ops::{alloc_fd, fd_check, fd_check_perm, is_kernel_boot};
pub(crate) use rw::*;
pub(crate) use seek::*;
pub(crate) use symlink::*;
pub(crate) use truncate::*;
pub(crate) use unlink::*;
pub(crate) use utimens::*;
pub(crate) use vnode::*;

pub(crate) use table::{fd_clear, fd_get, fd_set, fd_set_cloexec, fd_set_flags, fd_update_pos};
