//! Lua 5.1 runtime for OnyxOS
//!
//! Minimal Lua VM implementation for embedded systems.
//! Supports basic Lua features: tables, functions, closures.

pub mod lib;
#[cfg(target_arch = "riscv64")]
pub mod runtime;
pub mod value;
pub mod vm;

// Preserve the pre-`runtime/` public path (`luavm::repl`).
#[cfg(target_arch = "riscv64")]
pub use runtime::repl;
