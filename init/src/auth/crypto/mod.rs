// TODO(dead-code): auth::crypto::mod — shared auth/syscalls module, compiled per onyx_init bin;
// items unused by one binary are used by others (dead_code/unused_imports fire per-bin).
#![allow(dead_code, unused_imports)]

pub mod extra;
pub mod sha256;

pub use extra::{KDF_ITERS, bytes_to_hex, const_time_eq, hash_password};
pub(crate) use extra::{copy_slice, format_dec, parse_dec};
pub use extra::{generate_salt, legacy_hash_password};
pub use onyx_core::crypto::kdf::{
    HashScheme, ShadowField, classify_password, format_shadow_field, hex_decode_8,
    parse_shadow_field,
};
pub use sha256::*;
