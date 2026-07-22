//! Syscall handler with ACL (ring-aware dispatch).
//!
//! `handle` is the single entry point invoked from the trap handler. It
//! performs the ACL check via `syscall_allowed` and then dispatches to one
//! of the `sys_*` functions defined in the sibling modules
//! (`fs_sys`, `proc_sys`, `snap_sys`, `ring_sys`). User-pointer validation
//! goes through the shared `user_ptr_ok` helper exposed here.

mod acl;
mod dispatch;

pub use dispatch::handle;
pub(super) use dispatch::{parse_user_path, user_ptr_ok};
