//! CSR access via inline asm.
//! Functions return/take u64 on all targets. On 32-bit (rv32), CSRs are
//! 32-bit wide so the assembly operands use u32 with implicit casts.
use core::arch::asm;

macro_rules! csr_read_u64 {
    ($name:ident, $csr:literal) => {
        /// # Safety
        ///
        /// The hart must execute at a privilege level where the target CSR
        /// is accessible (otherwise the read raises an illegal-instruction
        /// exception); the access is hart-local and touches no memory.
        #[inline]
        pub unsafe fn $name() -> u64 { unsafe {
            // SAFETY: hart-local privileged CSR read per the RISC-V privileged spec (see fn contract).
            #[cfg(all(not(test), target_pointer_width = "64"))]
            { let v: u64; asm!(concat!("csrr {0}, ", $csr), out(reg) v, options(nomem, nostack)); v }
            #[cfg(all(not(test), target_pointer_width = "32"))]
            { let v: u32; asm!(concat!("csrr {0}, ", $csr), out(reg) v, options(nomem, nostack)); v as u64 }
            #[cfg(test)]
            { 0 }
        }}
    };
}
macro_rules! csr_write_u64 {
    ($name:ident, $csr:literal) => {
        /// # Safety
        ///
        /// The hart must execute at a privilege level where the target CSR
        /// is writable (otherwise the write raises an illegal-instruction
        /// exception), and the written value must be a legal encoding for
        /// that CSR; the access is hart-local and touches no memory.
        #[inline]
        pub unsafe fn $name(v: u64) { unsafe {
            // SAFETY: hart-local privileged CSR write per the RISC-V privileged spec (see fn contract).
            #[cfg(all(not(test), target_pointer_width = "64"))]
            asm!(concat!("csrw ", $csr, ", {0}"), in(reg) v, options(nomem, nostack));
            #[cfg(all(not(test), target_pointer_width = "32"))]
            asm!(concat!("csrw ", $csr, ", {0}"), in(reg) (v as u32), options(nomem, nostack));
            #[cfg(test)]
            { let _ = v; }
        }}
    };
}
macro_rules! csr_set_u64 {
    ($name:ident, $csr:literal) => {
        /// # Safety
        ///
        /// The hart must execute at a privilege level where the target CSR
        /// is accessible (otherwise the set raises an illegal-instruction
        /// exception) and the settable bits must be legal for that CSR;
        /// the access is hart-local and touches no memory.
        #[inline]
        pub unsafe fn $name(m: u64) { unsafe {
            // SAFETY: hart-local privileged CSR set per the RISC-V privileged spec (see fn contract).
            #[cfg(all(not(test), target_pointer_width = "64"))]
            asm!(concat!("csrs ", $csr, ", {0}"), in(reg) m, options(nomem, nostack));
            #[cfg(all(not(test), target_pointer_width = "32"))]
            asm!(concat!("csrs ", $csr, ", {0}"), in(reg) (m as u32), options(nomem, nostack));
            #[cfg(test)]
            { let _ = m; }
        }}
    };
}
macro_rules! csr_clear_u64 {
    ($name:ident, $csr:literal) => {
        /// # Safety
        ///
        /// The hart must execute at a privilege level where the target CSR
        /// is accessible (otherwise the clear raises an illegal-instruction
        /// exception) and the clearable bits must be legal for that CSR;
        /// the access is hart-local and touches no memory.
        #[inline]
        pub unsafe fn $name(m: u64) { unsafe {
            // SAFETY: hart-local privileged CSR clear per the RISC-V privileged spec (see fn contract).
            #[cfg(all(not(test), target_pointer_width = "64"))]
            asm!(concat!("csrc ", $csr, ", {0}"), in(reg) m, options(nomem, nostack));
            #[cfg(all(not(test), target_pointer_width = "32"))]
            asm!(concat!("csrc ", $csr, ", {0}"), in(reg) (m as u32), options(nomem, nostack));
            #[cfg(test)]
            { let _ = m; }
        }}
    };
}

csr_read_u64!(read_sstatus, "sstatus");
csr_write_u64!(write_sstatus, "sstatus");
csr_set_u64!(set_sstatus, "sstatus");
csr_clear_u64!(clear_sstatus, "sstatus");
csr_read_u64!(read_sepc, "sepc");
csr_write_u64!(write_sepc, "sepc");
csr_read_u64!(read_scause, "scause");
csr_read_u64!(read_stval, "stval");
csr_read_u64!(read_satp, "satp");
csr_write_u64!(write_satp, "satp");
csr_write_u64!(write_stvec, "stvec");
csr_read_u64!(read_sie, "sie");
csr_set_u64!(set_sie, "sie");
csr_clear_u64!(clear_sie, "sie");
csr_write_u64!(write_sscratch, "sscratch");
csr_write_u64!(write_scounteren, "scounteren");
csr_read_u64!(read_mhartid, "mhartid");

/// # Safety
///
/// Must execute at a privilege level where `sfence.vma` is permitted
/// (S- or M-mode); invalidates all address-translation entries for the
/// current hart only.
#[inline]
pub unsafe fn sfence_vma_all() {
    // SAFETY: hart-local TLB invalidation, legal in S-/M-mode per the fn contract.
    unsafe {
        #[cfg(not(test))]
        asm!("sfence.vma zero, zero", options(nostack));
    }
}
/// # Safety
///
/// Must execute at a privilege level where `sfence.vma` is permitted
/// (S- or M-mode); `va`/`asid` must identify the translations the caller
/// wants invalidated; affects the current hart only.
#[inline]
pub unsafe fn sfence_vma(va: u64, asid: u64) {
    // SAFETY: hart-local TLB invalidation for va/asid, legal per the fn contract.
    unsafe {
        #[cfg(all(not(test), target_pointer_width = "64"))]
        asm!("sfence.vma {0}, {1}", in(reg) va, in(reg) asid, options(nostack));
        #[cfg(all(not(test), target_pointer_width = "32"))]
        asm!("sfence.vma {0}, {1}", in(reg) (va as u32), in(reg) (asid as u32), options(nostack));
        #[cfg(test)]
        {
            let _ = (va, asid);
        }
    }
}
/// # Safety
///
/// Must execute at a privilege level where `wfi` does not trap
/// (S- or M-mode, i.e. TW=0); hart-local wait for interrupt, no memory access.
#[inline]
pub unsafe fn wfi() {
    // SAFETY: wfi is a hart-local wait-for-interrupt with no memory effects.
    unsafe {
        #[cfg(not(test))]
        asm!("wfi", options(nostack));
        #[cfg(test)]
        core::hint::spin_loop();
    }
}

csr_read_u64!(read_cycle, "cycle");
csr_read_u64!(read_time, "time");
csr_read_u64!(read_instret, "instret");
