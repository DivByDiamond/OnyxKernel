// TODO(2026-09-12): auth::shadow::mod — shared auth/syscalls module, compiled per onyx_init bin;
// Verified 2026-08-27: all items are live (each is used by at least one onyx_init
// bin; per-bin dead_code/unused_imports warnings are unavoidable without a lib
// target). Revisit if onyx_init gains a shared [lib] target.
#![allow(
    dead_code,
    reason = "TODO(2026-09-12): shared auth/syscalls/term module compiled per onyx_init bin; per-bin unused items are live in other bins"
)]
#![allow(
    unused_imports,
    reason = "TODO(2026-09-12): same shared-module per-bin import set"
)]

pub mod shadow_core;
pub mod shadow_io;

pub(crate) use shadow_core::format_shadow_entry;
pub use shadow_core::verify_shadow_password;
pub use shadow_core::{VerifyOutcome, read_shadow_password, verify_shadow_outcome};
pub use shadow_io::{delete_shadow_entry, update_shadow_password};
