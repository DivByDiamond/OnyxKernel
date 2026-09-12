//! Lua REPL binary

#![no_std]
#![no_main]
#![allow(
    unsafe_op_in_unsafe_fn,
    reason = "TODO(2026-09-13): syscalls::call::* asm! sites are now explicitly unsafe{}-wrapped (real fix); remaining warnings are this bin's own ~300 call sites into those wrappers, not yet individually wrapped"
)]

extern crate alloc;

mod kalloc_lua;
mod luavm;
mod syscalls; // global allocator

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
