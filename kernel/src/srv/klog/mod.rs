//! klog — formatted logging via UART.
//!
//! Split by responsibility: [`io`] (UART serialization lock, raw bytes and
//! the panic path), [`level`] (log level filter), [`macros`]
//! (kdbg!/kinf!/kwrn!/kerr!/kpanic!) and [`power`] (halt/reboot/shutdown).
mod io;
mod level;
mod macros;
mod power;

pub(crate) use io::UART_LOCK;

pub use io::{PanicWriter, debug_mark, emit, panic_handler, putc, puts};
pub use level::{Level, enabled, set_max_level};
pub use power::{halt, reset_machine};

pub use onyx_core::fmt::Arg as FmtArg;
