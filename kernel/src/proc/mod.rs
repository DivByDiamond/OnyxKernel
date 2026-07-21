//! Process management module.
//!
//! Re-exports the public surface from `process`, `lifecycle`, `spawn`,
//! `scheduler`, and `signals` submodules so callers can use
//! `crate::proc::current_pid()` etc. without picking a submodule.
pub mod lifecycle;
pub mod onx;
pub mod process;
pub mod scheduler;
pub mod signals;
pub mod spawn;

#[cfg(test)]
mod tests;

pub use lifecycle::*;
pub use process::*;
pub use scheduler::*;
pub use signals::*;
pub use spawn::*;
