//! File metadata operations: permissions, ownership, timestamps, size.
//!
//! `owner` groups the ownership operations (chmod/chown); truncate and
//! utimens sit at this level.
pub(crate) mod owner;
pub mod truncate;
pub mod utimens;

pub(crate) use owner::{chmod, chown};
