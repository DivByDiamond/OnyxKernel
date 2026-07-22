//! CSR access via inline asm.
//! Functions return/take u64 on all targets. On 32-bit (rv32), CSRs are
//! 32-bit wide so the assembly operands use u32 with implicit casts.
use core::arch::asm;

macro_rules! csr_read_u64 {
    ($name:ident, $csr:literal) => {
        #[inline]
        pub unsafe fn $name() -> u64 {
            #[cfg(all(not(test), target_pointer_width = "64"))]
            { let v: u64; asm!(concat!("csrr {0}, ", $csr), out(reg) v, options(nomem, nostack)); v }
            #[cfg(all(not(test), target_pointer_width = "32"))]
            { let v: u32; asm!(concat!("csrr {0}, ", $csr), out(reg) v, options(nomem, nostack)); v as u64 }
            #[cfg(test)]
            { 0 }
        }
    };
}
macro_rules! csr_write_u64 {
    ($name:ident, $csr:literal) => {
        #[inline]
        pub unsafe fn $name(v: u64) {
            #[cfg(all(not(test), target_pointer_width = "64"))]
            asm!(concat!("csrw ", $csr, ", {0}"), in(reg) v, options(nomem, nostack));
            #[cfg(all(not(test), target_pointer_width = "32"))]
            asm!(concat!("csrw ", $csr, ", {0}"), in(reg) (v as u32), options(nomem, nostack));
            #[cfg(test)]
            { let _ = v; }
        }
    };
}
macro_rules! csr_set_u64 {
    ($name:ident, $csr:literal) => {
        #[inline]
        pub unsafe fn $name(m: u64) {
            #[cfg(all(not(test), target_pointer_width = "64"))]
            asm!(concat!("csrs ", $csr, ", {0}"), in(reg) m, options(nomem, nostack));
            #[cfg(all(not(test), target_pointer_width = "32"))]
            asm!(concat!("csrs ", $csr, ", {0}"), in(reg) (m as u32), options(nomem, nostack));
            #[cfg(test)]
            { let _ = m; }
        }
    };
}
macro_rules! csr_clear_u64 {
    ($name:ident, $csr:literal) => {
        #[inline]
        pub unsafe fn $name(m: u64) {
            #[cfg(all(not(test), target_pointer_width = "64"))]
            asm!(concat!("csrc ", $csr, ", {0}"), in(reg) m, options(nomem, nostack));
            #[cfg(all(not(test), target_pointer_width = "32"))]
            asm!(concat!("csrc ", $csr, ", {0}"), in(reg) (m as u32), options(nomem, nostack));
            #[cfg(test)]
            { let _ = m; }
        }
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
csr_read_u64!(read_mhartid, "mhartid");

#[inline]
pub unsafe fn sfence_vma_all() {
    #[cfg(not(test))]
    asm!("sfence.vma zero, zero", options(nostack));
}
#[inline]
pub unsafe fn sfence_vma(va: u64, asid: u64) {
    #[cfg(all(not(test), target_pointer_width = "64"))]
    asm!("sfence.vma {0}, {1}", in(reg) va, in(reg) asid, options(nostack));
    #[cfg(all(not(test), target_pointer_width = "32"))]
    asm!("sfence.vma {0}, {1}", in(reg) (va as u32), in(reg) (asid as u32), options(nostack));
    #[cfg(test)]
    {
        let _ = (va, asid);
    }
}
#[inline]
pub unsafe fn wfi() {
    #[cfg(not(test))]
    asm!("wfi", options(nostack));
    #[cfg(test)]
    core::hint::spin_loop();
}

csr_read_u64!(read_cycle, "cycle");
csr_read_u64!(read_time, "time");
csr_read_u64!(read_instret, "instret");
