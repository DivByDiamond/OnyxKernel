//! Halt and reboot/shutdown primitives.

pub fn halt() -> ! {
    // SAFETY: S-mode CSR write clears SIE (stops same-hart timer preemption) then parks in a wfi loop; no memory access.
    unsafe {
        crate::arch::csr::clear_sstatus(crate::arch::regs::SSTATUS_SIE);
        loop {
            crate::arch::csr::wfi();
        }
    }
}

/// Reboot (`restart = true`) or power off the machine. Tries SBI SRST first
/// (real firmware: OpenSBI / OC2R); on failure falls back to the QEMU virt
/// sifive_test finisher — 0x5555 (FINISHER_PASS) exits QEMU for shutdown,
/// 0x7777 (FINISHER_RESET) resets the machine. Never returns: without any
/// reset mechanism (bare hardware, no SRST) the harts park in `halt()`.
pub fn reset_machine(restart: bool) -> ! {
    // `arch::sbi` is gated out under host `cargo test` builds (its ecall
    // uses riscv-only inline asm) — mirror that gate here so the SBI SRST
    // attempt is skipped there and only the QEMU finisher fallback runs.
    #[cfg(any(feature = "smode", all(not(test), target_pointer_width = "64")))]
    {
        let reset_type = if restart {
            crate::arch::sbi::SRST_COLD_REBOOT
        } else {
            crate::arch::sbi::SRST_SHUTDOWN
        };
        crate::arch::sbi::system_reset(reset_type);
    }
    // SAFETY: 0x100000 is the fixed QEMU-virt sifive_test finisher MMIO register; the volatile
    // word write requests shutdown/reset and bypasses compiler reordering.
    unsafe {
        let finisher = 0x100000usize as *mut u32;
        core::ptr::write_volatile(finisher, if restart { 0x7777 } else { 0x5555 });
    }
    halt()
}
