#![no_std]
#![no_main]

mod syscalls;

const MSG: &[u8] = b"Hello from Onyx!\n";

#[unsafe(no_mangle)]
/// # Safety
///
/// Process entry point: called directly by the kernel from the ELF entry
/// address; the stack is freshly initialized per the RISC-V calling convention.
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
