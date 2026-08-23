// TODO(dead-code): auth::group::mod — shared auth/syscalls module, compiled per onyx_init bin;
// items unused by one binary are used by others (dead_code/unused_imports fire per-bin).
#![allow(dead_code, unused_imports)]

pub mod file;
pub mod group_core;

pub(crate) use file::atomic_rewrite;
pub use group_core::{
    GroupEntry, find_group_by_gid, find_group_by_name, parse_group, read_groups, user_in_group,
};
