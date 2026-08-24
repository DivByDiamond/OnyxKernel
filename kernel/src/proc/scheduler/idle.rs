use core::sync::atomic::Ordering;

use crate::{
    arch::{csr, regs::SSTATUS_SIE},
    proc::process::{G_NEED_RESCHED, current_for_hart, hart_id},
    srv::timer,
};

pub unsafe fn is_idle() -> bool {
    unsafe { current_for_hart(hart_id()).is_null() }
}

pub unsafe fn sched_enter_idle() -> ! {
    unsafe {
        let hartid = hart_id();
        csr::write_stvec(crate::arch::asm::trap_entry as *const () as usize as u64);
        let _hartid = hartid;
        // sscratch = 0 in kernel mode (trap_entry discriminator). The idle loop
        // never enters user space, so the stack top lives only in sp.
        csr::write_sscratch(0);
        timer::init_hart(hartid);
        // ── SIE / spinlock invariant (audit note) ─────────────────────────────
        // trap_return (arch/asm/trap_asm.rs) unconditionally CLEARS sstatus.SIE
        // before returning to any context. That global invariant is what makes
        // the kernel's SpinLocks safe: a hart can never be preempted by a timer
        // tick while holding a spinlock, because no kernel context ever runs
        // with SIE set.
        //
        // The idle loop below is the ONE context that enables SIE — it does so
        // once here (timer + external interrupts for wfi wake-up) and then only
        // sleeps in wfi or executes the trap handler, which returns via
        // trap_return with SIE cleared again. It takes NO spinlocks in this
        // loop. If SIE is ever re-enabled for other idle-hart work (follow-up),
        // every SpinLock acquisition must first be proven to run with SIE off,
        // or the lock paths need an explicit local-irq-disable wrapper.
        csr::set_sie((1 << 5) | (1 << 9));
        csr::set_sstatus(SSTATUS_SIE);
        loop {
            G_NEED_RESCHED[hartid].store(false, Ordering::Release);
            csr::wfi();
        }
    }
}
