// TODO(dead-code): auth::shadow::mod — shared auth/syscalls module, compiled per onyx_init bin;
// items unused by one binary are used by others (dead_code/unused_imports fire per-bin).
#![allow(dead_code, unused_imports)]

pub mod shadow_core;
pub mod shadow_io;

pub(crate) use shadow_core::format_shadow_entry;
pub use shadow_core::{read_shadow_password, verify_shadow_password};
pub use shadow_io::{delete_shadow_entry, update_shadow_password};
