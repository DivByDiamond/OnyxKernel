#[cfg(not(test))]
use crate::arch::csr;
#[cfg(not(test))]
use onyx_core::fmt::{Arg, Write, vformat};

/// Print a panic-time diagnostic dump: hart id, trap CSRs and current
/// process state, via the panic writer.
///
/// # Safety
///
/// Intended for the panic/halt path only: must run in S-mode with
/// interrupts disabled, the kernel direct mapping live (CSR reads and
/// process-table access), and must not allocate.
pub unsafe fn kdump() {
    #[cfg(not(test))]
    // SAFETY: panic/halt path per the fn contract: the asm tp read and the
    // CSR reads are hart-local privileged operations; kdump is only called
    // with interrupts disabled on a live kernel mapping.
    unsafe {
        let mut w = crate::srv::klog::PanicWriter;
        w.write_str("\n--- KDUMP ---\n");

        let hartid: usize;
        // SAFETY: reads `tp`, which holds this hart's id per the kernel
        // ABI; a pure register move with no clobbers.
        core::arch::asm!("mv {}, tp", out(reg) hartid);
        let args: &[Arg] = &[Arg::from(hartid)];
        vformat(&mut w, "hartid=%d\n", args);

        let sepc = csr::read_sepc();
        let sstatus = csr::read_sstatus();
        let scause = csr::read_scause();
        let stval = csr::read_stval();
        let satp = csr::read_satp();
        let sie = csr::read_sie();
        let args: &[Arg] = &[
            Arg::from(sepc),
            Arg::from(sstatus),
            Arg::from(scause),
            Arg::from(stval),
            Arg::from(satp),
            Arg::from(sie),
        ];
        vformat(
            &mut w,
            "sepc=%p sstatus=%p scause=%p stval=%p satp=%p sie=%p\n",
            args,
        );

        let pid = crate::proc::current_pid();
        if pid != 0 {
            let args: &[Arg] = &[Arg::from(pid)];
            vformat(&mut w, "pid=%d\n", args);
        }
        if let Some(p) = crate::proc::current_opt() {
            let args: &[Arg] = &[Arg::from(p.ring), Arg::from(p.parent_pid)];
            vformat(&mut w, "ring=%d parent=%d\n", args);
        }

        let cnt = crate::proc::count();
        let args: &[Arg] = &[Arg::from(cnt)];
        vformat(&mut w, "processes=%d\n", args);

        let online = crate::arch::smp::online_harts();
        let args: &[Arg] = &[Arg::from(online)];
        vformat(&mut w, "online_harts=%d\n", args);

        // Root-cause fix (SMP crash, todo.md "Отдельный SMP-краш под
        // -smp 2"): a frame-pointer backtrace used to run here, walking
        // `s0` as an fp-chain (`[fp-8]=ra`, `[fp-16]=old fp`). This project
        // builds with `-C force-frame-pointers=no` (.cargo/config.toml), so
        // `s0` is an ordinary scratch register with no guaranteed fp-chain
        // contents — dereferencing it as one is undefined behavior in
        // general and, live, reliably produced a SECOND, unrelated page
        // fault (garbage `s0` treated as a stack pointer) while reporting
        // the FIRST fault, turning one clean diagnostic into a confusing
        // cascade that looked like the SMP crash itself. Removed rather
        // than guarded more defensively: no amount of bounds-checking makes
        // a nonexistent fp-chain correct, only build with
        // force-frame-pointers=yes would.
        w.write_str("--- END KDUMP ---\n");
    }
}
