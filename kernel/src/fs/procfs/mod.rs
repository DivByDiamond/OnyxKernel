//! procfs — synthetic read-only files under /proc.
mod content;
mod dir;
mod fmt;

pub use content::read;
pub use dir::*;
pub use fmt::*;
