pub mod file;
pub mod group_core;

pub(crate) use file::atomic_rewrite;
pub use group_core::{GroupEntry, parse_group, read_groups, user_in_group};
