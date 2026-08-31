//! Lua REPL binary

#![no_std]
#![no_main]
// TODO(2026-08-31): bin-root allow — raw syscall asm runs inside `unsafe fn`
// wrappers (no_std, per-bin compile); re-evaluate on toolchain/edition bump.
#![allow(unsafe_op_in_unsafe_fn)]

extern crate alloc;

mod luavm;
mod syscalls;
mod kalloc_lua; // global allocator

use luavm::{VM, lib};

#[unsafe(no_mangle)]
/// # Safety
///
/// Process entry point: called directly by the kernel from the ELF entry
/// address; the stack is freshly initialized per the RISC-V calling convention.
pub unsafe extern "C" fn _start() -> ! {
    let mut vm = VM::new();
    lib::register_all(&mut vm);

    // TODO: Start REPL
    // For now, just exit
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
