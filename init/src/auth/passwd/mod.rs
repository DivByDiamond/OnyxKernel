// TODO(dead-code): auth::passwd::mod — shared auth/syscalls module, compiled per onyx_init bin;
// Verified 2026-08-27: all items are live (each is used by at least one onyx_init
// bin; per-bin dead_code/unused_imports warnings are unavoidable without a lib
// target). Revisit if onyx_init gains a shared [lib] target.
#![allow(dead_code, unused_imports)]

pub mod passwd_core;
pub mod passwd_io;

pub(crate) use passwd_core::format_passwd_entry;
pub use passwd_core::{PasswdEntry, find_user, find_user_by_uid, parse_passwd};
pub use passwd_io::{delete_passwd_entry, read_passwd, update_passwd_entry};
