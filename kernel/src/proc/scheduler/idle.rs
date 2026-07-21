use crate::arch::csr;
use crate::arch::regs::SSTATUS_SIE;
use crate::arch::smp::{G_SEC_STACKS, SEC_STACK_SIZE};
use crate::proc::process::{current_for_hart, hart_id, G_NEED_RESCHED};
use crate::srv::timer;
use core::sync::atomic::Ordering;

pub unsafe fn is_idle() -> bool {
    current_for_hart(hart_id()).is_null()
}

pub unsafe fn sched_enter_idle() -> ! {
    let hartid = hart_id();
    csr::write_stvec(crate::arch::asm::trap_entry as *const () as usize as u64);
    let stack_top = G_SEC_STACKS.as_ptr() as usize + (hartid + 1) * SEC_STACK_SIZE;
    csr::write_sscratch(stack_top as u64);
    timer::init_hart(hartid);
    csr::set_sie((1 << 5) | (1 << 9));
    csr::set_sstatus(SSTATUS_SIE);
    loop {
        G_NEED_RESCHED[hartid].store(false, Ordering::Release);
        csr::wfi();
    }
}
