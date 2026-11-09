//! OnyxFS v2 on-disk structs — `superblock`, `inode` and `dirent` live under
//! `disk/` to keep this folder small. All types are re-exported from here so
//! callers can use `onyx_core::formats::OnyfsSuper` etc.
pub mod disk;
pub mod journal;

pub use disk::*;
pub use journal::*;
