#![expect(dead_code)]

pub mod shadow_core;
pub mod shadow_io;

pub(crate) use shadow_core::format_shadow_entry;
pub use shadow_core::{read_shadow_password, verify_shadow_password};
pub use shadow_io::{delete_shadow_entry, update_shadow_password};
