//! Lua 5.1 runtime for OnyxOS
//!
//! Minimal Lua VM implementation for embedded systems.
//! Supports basic Lua features: tables, functions, closures.

pub mod lib;
pub mod runtime;
pub mod value;
pub mod vm;

// Preserve the pre-`runtime/` public path (`luavm::repl`).
pub use runtime::repl;
