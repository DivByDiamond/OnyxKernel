//! Kernel panic handler — dumps state to UART, bypassing the normal emit() path.
use super::lock::UART_LOCK;
use crate::drivers::uart;
use core::panic::PanicInfo;
use onyx_core::fmt::{Arg, Write, vformat};

pub struct PanicWriter;
impl onyx_core::fmt::Write for PanicWriter {
    fn write_str(&mut self, s: &str) {
        for &b in s.as_bytes() {
            if b == b'\n' {
                uart::putc(b'\r');
            }
            uart::putc(b);
        }
    }
}

fn delay_loops(n: u64) {
    for _ in 0..n {
        // SAFETY: bare `nop` asm -- no memory access and no registers declared as clobbered.
        unsafe { core::arch::asm!("nop") }
    }
}

pub fn panic_handler(info: &PanicInfo) -> ! {
    // Panics bypassed UART_LOCK entirely (PanicWriter writes uart::putc
    // directly), so a panic on one hart while another hart was mid-emit()
    // interleaved byte-by-byte with it — the exact garbling that made these
    // SMP crash logs unreadable. Never returns, so no matching unlock is
    // needed; a panic while THIS hart already holds the lock (inside its
    // own emit() call) would self-deadlock, but no kerr!/kinf! call in this
    // codebase panics before returning, so that path is not reachable.
    UART_LOCK.lock();
    let mut w = PanicWriter;
    w.write_str("\n\n*** KERNEL PANIC ***\n");
    if let Some(loc) = info.location() {
        let args: &[Arg] = &[
            Arg::from(loc.file()),
            Arg::from(loc.line()),
            Arg::from(loc.column()),
        ];
        vformat(&mut w, "  at %s:%d:%d\n", args);
    }
    // TODO(dead-code): `core::panic::PanicInfo::payload` is deprecated
    // upstream (pending PanicHookInfo migration); the &str downcast remains
    // the only no_std-compatible way to read a string panic message.
    // Revisit on a toolchain bump. 2026-08-24
    #[expect(deprecated)]
    if let Some(msg) = info.payload().downcast_ref::<&str>() {
        w.write_str("  msg: ");
        w.write_str(msg);
        w.write_char(b'\n');
    }
    // SAFETY: panic path -- kdump only reads CSRs and process state (volatile reads) and prints; no allocation.
    unsafe {
        crate::srv::kdump::kdump();
    }
    w.write_str("\n  Active processes:\n");
    crate::proc::dump_all(&mut w);
    w.write_str("\n  Rebooting in 3 seconds...\n");
    delay_loops(300_000_000);
    super::power::reset_machine(false)
}
