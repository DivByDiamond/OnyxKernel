//! Lua REPL binary

#![no_std]
#![no_main]

extern crate alloc;

mod kalloc_lua;
mod luavm;
use onyx_init::syscalls;

use luavm::repl;

#[unsafe(no_mangle)]
/// # Safety
///
/// Process entry point: called directly by the kernel from the ELF entry
/// address; the stack is freshly initialized per the RISC-V calling convention.
pub unsafe extern "C" fn _start() -> ! {
    repl::run_repl();
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {
        unsafe {
            core::arch::asm!("wfi");
        }
    }
}
