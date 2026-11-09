#![no_std]
#![no_main]

use core::arch::asm;

#[path = "../../auth/mod.rs"]
mod auth;
#[path = "../../passwd_flow/mod.rs"]
mod passwd_flow;
use onyx_init::syscalls;

use passwd_flow::root_flow::do_root_passwd;
use passwd_flow::user_flow::do_user_passwd;

#[unsafe(no_mangle)]
/// # Safety
///
/// Process entry point: called directly by the kernel from the ELF entry
/// address; the stack is freshly initialized per the RISC-V calling convention.
pub unsafe extern "C" fn _start() -> ! {
    unsafe {
        let ring = syscalls::getring();

        if ring == 2 {
            do_user_passwd();
        } else {
            do_root_passwd();
        }

        syscalls::exit(0);
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
