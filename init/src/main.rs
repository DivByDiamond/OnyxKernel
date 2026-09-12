#![no_std]
#![no_main]
#![allow(
    unsafe_op_in_unsafe_fn,
    reason = "TODO(2026-09-13): syscalls::call::* asm! sites are now explicitly unsafe{}-wrapped (real fix); remaining warnings are this bin's own ~300 call sites into those wrappers, not yet individually wrapped"
)]
#![warn(clippy::all)]

use core::arch::asm;

mod boottest;
mod pid1;
mod syscalls;
mod util;

const BANNER: &[u8] = b"[init] OnyxOS init v0.4 (service manager)\n";

#[unsafe(no_mangle)]
/// # Safety
///
/// Process entry point: called directly by the kernel from the ELF entry
/// address; the stack is freshly initialized per the RISC-V calling convention.
pub unsafe extern "C" fn _start(argc: usize, argv: *const u64, _envp: *const u64) -> ! {
    syscalls::write(1, BANNER.as_ptr(), BANNER.len());

    if argc > 0 {
        pid1::exec::ctl::control_main(argc, argv);
    } else {
        pid1::pid1_main();
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {
        unsafe {
            asm!("wfi");
        }
    }
}
