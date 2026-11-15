pub mod passwd_core;
pub mod passwd_io;

pub use passwd_core::{PasswdEntry, find_user, find_user_by_uid, parse_passwd};
pub use passwd_io::{delete_passwd_entry, read_passwd, update_passwd_entry};
