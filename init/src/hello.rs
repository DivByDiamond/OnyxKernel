#![no_std]
#![no_main]
#![allow(clippy::missing_safety_doc, non_snake_case, unsafe_op_in_unsafe_fn)]

mod syscalls;

const MSG: &[u8] = b"Hello from Onyx!\n";

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _start() -> ! {
    unsafe {
        syscalls::write(1, MSG.as_ptr(), MSG.len());
        syscalls::exit(0);
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {
        unsafe {
            core::arch::asm!("wfi");
        }
    }
}
