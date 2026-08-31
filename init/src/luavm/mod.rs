//! Lua 5.1 runtime for OnyxOS
//!
//! Minimal Lua VM implementation for embedded systems.
//! Supports basic Lua features: tables, functions, closures.

pub mod value;
pub mod vm;
pub mod lib;
pub mod syscall;
pub mod repl;

pub use value::Value;
pub use vm::VM;
