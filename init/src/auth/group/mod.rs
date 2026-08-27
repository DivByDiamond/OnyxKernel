// TODO(dead-code): auth::group::mod — shared auth/syscalls module, compiled per onyx_init bin;
// Verified 2026-08-27: all items are live (each is used by at least one onyx_init
// bin; per-bin dead_code/unused_imports warnings are unavoidable without a lib
// target). Revisit if onyx_init gains a shared [lib] target.
#![allow(dead_code, unused_imports)]

pub mod file;
pub mod group_core;

pub(crate) use file::atomic_rewrite;
pub use group_core::{GroupEntry, parse_group, read_groups, user_in_group};
