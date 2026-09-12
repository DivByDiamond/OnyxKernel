//! arch — RISC-V 64 architecture-specific layer.

pub mod asm;
pub mod bits;
pub mod csr;
pub mod mmio;
pub mod regs;
// Needed by every real (non-test) build now that rv32 also arms its timer
// via the legacy SBI_SET_TIMER ecall (arch::asm::mtrap_32, 2026-09-12) —
// previously this only mattered for smode and real rv64.
#[cfg(any(feature = "smode", not(test)))]
pub mod sbi;
pub mod smp;
pub mod trap_frame;

pub use regs::*;

// SAFETY: the kernel linker script defines these symbols as kernel image
// boundary markers; they exist in every final link and only their addresses
// are ever taken (never dereferenced as `u8`).
unsafe extern "Rust" {
    pub static __bss_start: u8;
    pub static __bss_end: u8;
    pub static __stack_top: u8;
    pub static __stack_bottom: u8;
    pub static __kernel_end: u8;
}

#[unsafe(no_mangle)]
pub static SAVED_HARTID: usize = 0;
#[unsafe(no_mangle)]
pub static SAVED_FDT: usize = 0;
