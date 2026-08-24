use crate::arch::csr;
use onyx_core::fmt::{Arg, Write, vformat};

const MAX_BT_DEPTH: usize = 64;

#[cfg(not(test))]
/// Walk the RISC-V frame-pointer chain and print `ra` per frame.
///
/// # Safety
///
/// Must run on a kernel thread whose frames follow the standard fp-chain
/// layout (`[fp-16] = saved s0/fp`, `[fp-8] = ra`); corrupted chains are
/// tolerated (null/misaligned fp or ra terminate the walk) but the initial
/// `s0` must at least be a readable stack address.
unsafe fn backtrace(w: &mut impl Write) {
    // SAFETY: walks the fp chain guarded by the null/alignment checks; the
    // two word reads per frame target stack slots per the `# Safety`
    // contract, and the asm only reads s0.
    unsafe {
        let mut fp: usize;
        // SAFETY: reads the current frame pointer register s0 via a pure move;
        // no memory access, no clobbers beyond the declared `out(reg)` output.
        core::arch::asm!("mv {}, s0", out(reg) fp);
        // Saved-ra / saved-fp slots sit just below the frame pointer at
        // native word size (8 bytes on rv64, 4 on rv32).
        const WORD: usize = core::mem::size_of::<usize>();
        for i in 0..MAX_BT_DEPTH {
            if fp == 0 || fp & 0xf != 0 {
                break;
            }
            let ra = *((fp - WORD) as *const usize);
            let old_fp = *((fp - 2 * WORD) as *const usize);
            let args: &[Arg] = &[Arg::from(i), Arg::from(ra)];
            vformat(w, "  [%d] ra=%p\n", args);
            if ra == 0 {
                break;
            }
            fp = old_fp;
        }
    }
}

/// Print a panic-time diagnostic dump: hart id, trap CSRs, current process
/// state and a frame-pointer backtrace, via the panic writer.
///
/// # Safety
///
/// Intended for the panic/halt path only: must run in S-mode with
/// interrupts disabled, the kernel direct mapping live (CSR reads and
/// process-table access), and must not allocate.
pub unsafe fn kdump() {
    unsafe {
        #[cfg(not(test))]
        {
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

            w.write_str("Backtrace:\n");
            backtrace(&mut w);

            w.write_str("--- END KDUMP ---\n");
        }
    }
}
