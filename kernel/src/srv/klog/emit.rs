//! Raw UART byte output and the formatted-line emit path used by kinf!/kwrn!/etc.
use super::level::{Level, enabled};
use super::lock::UART_LOCK;
use crate::drivers::uart;
use core::sync::atomic::{AtomicU32, Ordering};
use onyx_core::fmt::{Arg, Write, vformat};

pub(crate) struct UartWriter;
impl Write for UartWriter {
    fn write_str(&mut self, s: &str) {
        for &b in s.as_bytes() {
            if b == b'\n' {
                uart::putc(b'\r');
            }
            uart::putc(b);
        }
    }
    fn write_char(&mut self, c: u8) {
        if c == b'\n' {
            uart::putc(b'\r');
        }
        uart::putc(c);
    }
}

pub fn debug_mark(c: u8) {
    // Host-test guard: boot-progress marks write UART MMIO (0x1000_0000),
    // which would fault the host test process. Compile them out under
    // cfg(test) so boot-path code (e.g. fdt::init_from rejection marks)
    // stays unit-testable.
    #[cfg(test)]
    {
        let _ = c;
    }
    #[cfg(not(test))]
    {
        uart::putc(b'[');
        uart::putc(c);
        uart::putc(b']');
    }
}

pub fn puts(s: &str) {
    for &b in s.as_bytes() {
        if b == b'\n' {
            uart::putc(b'\r');
        }
        uart::putc(b);
    }
}
pub fn putc(c: u8) {
    if c == b'\n' {
        uart::putc(b'\r');
    }
    uart::putc(c);
}

/// Diagnostic-only monotonic print sequence number, prefixed to every
/// emitted line while chasing the residual SMP fault (todo.md). Lets a
/// still-garbled capture be checked for whether bytes from two DIFFERENT
/// sequence numbers are truly interleaved mid-line (a real cross-hart UART
/// race despite UART_LOCK) versus the host terminal/QEMU chardev merely
/// reordering otherwise-intact, correctly-serialized lines.
static EMIT_SEQ: AtomicU32 = AtomicU32::new(0);

pub fn emit(level: Level, tag: &str, fmt: &str, args: &[Arg]) {
    if !enabled(level) {
        return;
    }
    UART_LOCK.lock();
    let seq = EMIT_SEQ.fetch_add(1, Ordering::Relaxed);
    let hart = crate::proc::process::hart_id();
    let mut w = UartWriter;
    vformat(
        &mut w,
        "#%d/h%d ",
        &[Arg::from(seq), Arg::from(hart as u32)],
    );
    w.write_char(b'[');
    w.write_str(level.as_str());
    w.write_char(b']');
    w.write_char(b' ');
    w.write_str(tag);
    w.write_str(": ");
    vformat(&mut w, fmt, args);
    w.write_char(b'\n');
    UART_LOCK.unlock();
}
