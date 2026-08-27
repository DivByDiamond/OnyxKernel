// TODO(dead-code): auth::crypto::mod — shared auth/syscalls module, compiled per onyx_init bin;
// Verified 2026-08-27: all items are live (each is used by at least one onyx_init
// bin; per-bin dead_code/unused_imports warnings are unavoidable without a lib
// target). Revisit if onyx_init gains a shared [lib] target.
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
