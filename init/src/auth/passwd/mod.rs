#![expect(dead_code)]

pub mod passwd_core;
pub mod passwd_io;

pub(crate) use passwd_core::format_passwd_entry;
pub use passwd_core::{find_user, find_user_by_uid, parse_passwd, PasswdEntry};
pub use passwd_io::{delete_passwd_entry, read_passwd, update_passwd_entry};
