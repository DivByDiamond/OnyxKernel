use crate::arch::csr;
use crate::arch::regs::SSTATUS_SIE;
use crate::proc::process::{G_NEED_RESCHED, current_for_hart, hart_id};
use crate::srv::timer;
use core::sync::atomic::Ordering;

pub unsafe fn is_idle() -> bool { unsafe {
    current_for_hart(hart_id()).is_null()
}}

pub unsafe fn sched_enter_idle() -> ! { unsafe {
    let hartid = hart_id();
    csr::write_stvec(crate::arch::asm::trap_entry as *const () as usize as u64);
    let _hartid = hartid;
    // sscratch = 0 in kernel mode (trap_entry discriminator). The idle loop
    // never enters user space, so the stack top lives only in sp.
    csr::write_sscratch(0);
    timer::init_hart(hartid);
    csr::set_sie((1 << 5) | (1 << 9));
    csr::set_sstatus(SSTATUS_SIE);
    loop {
        G_NEED_RESCHED[hartid].store(false, Ordering::Release);
        csr::wfi();
    }
}}
