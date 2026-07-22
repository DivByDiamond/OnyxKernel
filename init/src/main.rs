#![no_std]
#![no_main]
#![warn(clippy::all)]
#![allow(clippy::missing_safety_doc, unsafe_op_in_unsafe_fn, non_snake_case)]

use core::arch::asm;

mod boottest;
mod pid1;
mod syscalls;
mod util;

const BANNER: &[u8] = b"[init] OnyxOS init v0.4 (service manager)\n";

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _start(argc: usize, argv: *const u64) -> ! {
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
