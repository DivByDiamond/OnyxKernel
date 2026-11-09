//! OnyxFS namespace operations.
//!
//! `dir` groups directory-level mutations (mkdir, rename) and listing;
//! `file` groups regular-file read plus symlink/unlink entry handling.
pub(crate) mod dir;
pub(crate) mod file;

pub use dir::*;
pub use file::*;
