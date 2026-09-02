#![no_std]
#![no_main]
// TODO(2026-08-27): bin-root allow — raw syscall asm runs inside `unsafe fn`
// wrappers (no_std, per-bin compile); re-evaluate on toolchain/edition bump.
#![allow(unsafe_op_in_unsafe_fn)]

mod syscalls;

fn write_dec(v: u64) {
    let mut buf = [0u8; 20];
    let mut p = 20;
    let mut n = v;
    if n == 0 {
        buf[19] = b'0';
        unsafe {
            syscalls::write(1, buf[19..].as_ptr(), 1);
        }
        return;
    }
    while n > 0 && p > 0 {
        p -= 1;
        buf[p] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    unsafe {
        syscalls::write(1, buf[p..].as_ptr(), 20 - p);
    }
}

fn puts(s: &[u8]) {
    unsafe {
        syscalls::write(1, s.as_ptr(), s.len());
    }
}

/// Diagnostic for the `usleep()`/`nanosleep()` hang (todo.md): prints
/// uptime before and after a 500ms `nanosleep()`. As of 2026-09-02 this
/// still hangs after "before" — see todo.md for the root-caused platform
/// issue (timer interrupts never reach S-mode in this QEMU boot chain).
/// Kept as a ready-made repro for whoever picks that fix up next.
/// # Safety
///
/// Process entry point: called directly by the kernel with `a0`/`a1` as the
/// raw argument registers; unused here.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn _start(_a0: usize, _a1: usize) -> ! {
    let mut ts: [u64; 2] = [0, 0];
    unsafe {
        syscalls::clock_gettime(0, ts.as_mut_ptr());
    }
    puts(b"sleep_test: before uptime_us=");
    write_dec(ts[0] * 1_000_000 + ts[1] / 1000);
    puts(b"\n");

    puts(b"sleep_test: about to nanosleep\n");
    let req: [u64; 2] = [0, 500_000_000]; // 500ms
    let r = unsafe { syscalls::nanosleep(req.as_ptr(), core::ptr::null_mut()) };
    puts(b"sleep_test: nanosleep call returned to userspace\n");
    puts(b"sleep_test: nanosleep returned ");
    write_dec(if r < 0 { (-r) as u64 } else { r as u64 });
    puts(b"\n");

    unsafe {
        syscalls::clock_gettime(0, ts.as_mut_ptr());
    }
    puts(b"sleep_test: after uptime_us=");
    write_dec(ts[0] * 1_000_000 + ts[1] / 1000);
    puts(b"\n");
    puts(b"sleep_test: DONE\n");

    syscalls::exit(0);
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {
        unsafe {
            core::arch::asm!("wfi");
        }
    }
}
