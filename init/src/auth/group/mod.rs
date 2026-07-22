#![expect(dead_code)]

pub mod file;
pub mod group_core;

pub(crate) use file::atomic_rewrite;
pub use group_core::{
    find_group_by_gid, find_group_by_name, parse_group, read_groups, user_in_group, GroupEntry,
};
