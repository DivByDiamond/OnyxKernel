//! Lua 5.1 runtime for OnyxOS
//!
//! Minimal Lua VM implementation for embedded systems.
//! Supports basic Lua features: tables, functions, closures.

pub mod lib;
pub mod repl;
pub mod syscall;
pub mod value;
pub mod vm;
