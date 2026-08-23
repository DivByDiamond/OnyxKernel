// TODO(dead-code): auth::passwd::mod — shared auth/syscalls module, compiled per onyx_init bin;
// items unused by one binary are used by others (dead_code/unused_imports fire per-bin).
#![allow(dead_code, unused_imports)]

pub mod passwd_core;
pub mod passwd_io;

pub(crate) use passwd_core::format_passwd_entry;
pub use passwd_core::{PasswdEntry, find_user, find_user_by_uid, parse_passwd};
pub use passwd_io::{delete_passwd_entry, read_passwd, update_passwd_entry};
