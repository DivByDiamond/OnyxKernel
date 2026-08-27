//! Inode metadata write paths: file creation and truncation.
//!
//! Responsibility split:
//! - [`create`]: file creation (inode allocation + dirent + journal commit).
//! - [`truncate`]: truncate-to-zero, truncate-to-length (shrink) and the
//!   shared block-freeing helper.
//! - [`extend`]: zero-filled block allocation for truncate-to-length growth.

mod create;
mod extend;
mod truncate;

pub use create::create;
pub use truncate::{truncate, truncate_to_length};
