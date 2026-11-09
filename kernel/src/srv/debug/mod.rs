//! Diagnostics helpers: the panic-time state dump and the syscall dispatch
//! re-export used by the trap path.

pub mod kdump;
pub mod syscall_dispatch;
