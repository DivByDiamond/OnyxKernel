//! Raw UART output plumbing: the byte writers, the reentrant UART lock that
//! serializes lines across harts, and the panic path that bypasses emit().

mod emit;
mod lock;
mod panic;

pub(crate) use lock::UART_LOCK;

pub use emit::{debug_mark, emit, putc, puts};
pub use panic::{PanicWriter, panic_handler};
