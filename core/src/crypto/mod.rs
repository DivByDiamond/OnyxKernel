//! Pure cryptography helpers shared by the kernel, init userspace and host
//! tests. Everything here is `no_std`, allocation-free and deterministic so
//! it can be unit-tested on the host via `cargo test -p onyx_core`.
//!
//! NOTE: this is a deliberately small hand-rolled toolkit for an embedded
//! appliance — not a general-purpose crypto library.

pub mod kdf;
pub mod sha256;

pub use kdf::*;
pub use sha256::*;
