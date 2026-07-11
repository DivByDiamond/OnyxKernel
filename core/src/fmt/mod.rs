//! Formatting engine — split from the old monolithic `fmt.rs`.
pub mod format_num;
pub mod impls;
pub mod writer;

pub use impls::Arg;
pub use writer::{Write, vformat};
