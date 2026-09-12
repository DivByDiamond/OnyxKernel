//! klog — formatted logging via UART.
//!
//! Split by responsibility: [`lock`] (UART serialization lock), [`level`]
//! (log level filter), [`emit`] (raw UART bytes + the formatted-line emit
//! path used by the macros), [`macros`] (kdbg!/kinf!/kwrn!/kerr!/kpanic!),
//! [`panic`] (kernel panic handler) and [`power`] (halt/reboot/shutdown).
mod emit;
mod level;
mod lock;
mod macros;
mod panic;
mod power;

pub(crate) use lock::UART_LOCK;

pub use emit::{debug_mark, emit, putc, puts};
pub use level::{Level, enabled, set_max_level};
pub use panic::{PanicWriter, panic_handler};
pub use power::{halt, reset_machine};

pub use onyx_core::fmt::Arg as FmtArg;
