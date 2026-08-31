//! Secondary-hart boot path: `secondary_continue` trampoline and
//! `secondary_kmain` entry. Split from `super` so each file stays within
//! the project's 250-line limit.
#[cfg(not(test))]
use super::{G_KERNEL_ROOT_PA, G_ONLINE_HARTS, G_SEC_STACKS, SEC_STACK_SIZE};
#[cfg(not(test))]
use core::sync::atomic::Ordering;

/// Common continuation once a secondary hart is allowed to run: switch to
/// its private stack, load the kernel root page table and drop to S-mode in
/// `secondary_kmain`. Reached either from `secondary_entry` (firmware hands
/// every hart to the kernel) or directly via the bootloader SMP mailbox.
///
/// # Safety
///
/// Must only be invoked by the secondary-hart boot protocol: `tp` holds a
/// hartid < MAX_HARTS (set by entry asm), the hart is still in M-mode (or
/// the `smode` feature build parks it in S-mode with satp unset), and
/// `G_SEC_STACKS`/`G_KERNEL_ROOT_PA` were initialized before the hart was
/// released (bootloader mailbox SeqCst publish / `G_RELEASE` Acquire pair).
#[cfg(not(test))]
pub unsafe extern "Rust" fn secondary_continue() -> ! {
    // SAFETY: G_SEC_STACKS/G_KERNEL_ROOT_PA are aligned program-lifetime statics; tp holds a hartid < MAX_HARTS (set by entry asm); asm operands are register-width and the asm never returns.
    unsafe {
        let hartid: usize;
        core::arch::asm!("mv {0}, tp", out(reg) hartid);
        let sp = &raw const G_SEC_STACKS as *const u8 as usize + (hartid + 1) * SEC_STACK_SIZE;
        let entry = secondary_kmain as *const () as usize;
        // SAFETY: G_KERNEL_ROOT_PA is an aligned u64 static that outlives the program; read_volatile pairs with the publishing store.
        let root_pa = core::ptr::read_volatile(&raw const G_KERNEL_ROOT_PA);
        // Register-width typed so `in(reg)` operands match the target's
        // integer register class (u64 on rv64, u32 on rv32).
        #[cfg(target_pointer_width = "64")]
        let satp: usize = if root_pa != 0 {
            (8usize << 60) | ((root_pa >> 12) as usize)
        } else {
            0
        };
        #[cfg(target_pointer_width = "32")]
        let satp: usize = if root_pa != 0 {
            (crate::arch::bits::SATP_MODE_SV32 as usize) | (((root_pa >> 12) & 0x3FF_FFFF) as usize)
        } else {
            0
        };
        #[cfg(not(feature = "smode"))]
        core::arch::asm!(
            // This hart entered through the bootloader SMP mailbox (or the
            // `park` spin in boot.rs), so it NEVER ran the kernel's `_start`
            // bootstrap — and therefore lacks the per-hart machine CSRs that
            // `_start` programs on hart 0. We are still in M-mode here, so
            // mirror that bootstrap now, before dropping to S-mode:
            //
            //   * PMP entry 0 (TOR, R|W|X over the first 1 GiB): with no
            //     matching PMP entry, S/U-mode access to ALL memory faults,
            //     so the first S-mode instruction fetch after `mret` below
            //     would die with an instruction access fault (observed on
            //     QEMU virt as pc=0, mcause=1, because mtvec is still 0).
            //   * medeleg/mideleg: route page faults, misaligned/access
            //     faults and S-mode ecalls/interrupts to the kernel trap
            //     handler instead of an unhandled M-mode trap.
            //   * mcounteren: allow lower modes to read cycle/time/instret
            //     (`rdtime` in the timer code raises illegal-instruction
            //     otherwise).
            "li t0, 0x3FFFFFFF",
            "csrw pmpaddr0, t0",
            "li t0, 0x9F",
            "csrw pmpcfg0, t0",
            "li t0, (1<<0)|(1<<1)|(1<<2)|(1<<3)|(1<<5)|(1<<7)|(1<<8)|(1<<9)|(1<<11)|(1<<12)|(1<<13)|(1<<15)",
            "csrw medeleg, t0",
            "li t0, (1<<1)|(1<<5)|(1<<9)",
            "csrw mideleg, t0",
            "li t0, (1<<0)|(1<<1)|(1<<2)",
            "csrw mcounteren, t0",
            "mv sp, {0}",
            "csrw mepc, {1}",
            "li t0, 1 << 11",
            "csrs mstatus, t0",
            "li t0, 1 << 12",
            "csrc mstatus, t0",
            "li t0, 1 << 7",
            "csrc mstatus, t0",
            "csrw satp, {2}",
            "sfence.vma zero, zero",
            "mret",
            in(reg) sp,
            in(reg) entry,
            in(reg) satp,
            options(noreturn),
        );
        #[cfg(feature = "smode")]
        core::arch::asm!(
            "mv sp, {0}",
            "csrw sepc, {1}",
            "li t0, 1 << 8",     // sstatus.SPP = 1
            "csrs sstatus, t0",
            "li t0, 1 << 5",     // sstatus.SPIE = 1
            "csrs sstatus, t0",
            "csrw satp, {2}",
            "sfence.vma zero, zero",
            "sret",
            in(reg) sp,
            in(reg) entry,
            in(reg) satp,
            options(noreturn),
        );
    }
}

#[cfg(test)]
#[unsafe(no_mangle)]
/// # Safety
///
/// Test stub: never runs; the crate is not booted under `cargo test`.
pub unsafe extern "Rust" fn secondary_entry() -> ! {
    loop {
        // Divergence point: spin (never returns) without a busy no-op loop.
        core::hint::spin_loop();
    }
}

// Test twin of the real `secondary_continue`: `release_secondary_harts`
// publishes its address into the bootloader mailbox unconditionally, so the
// symbol must exist when the crate is built for `cargo test` (where the
// real M-mode/S-mode trampoline is compiled out).
#[cfg(test)]
/// # Safety
///
/// Test stub: never runs; exists so the mailbox symbol link resolves.
pub unsafe extern "Rust" fn secondary_continue() -> ! {
    loop {
        // Divergence point: spin (never returns) without a busy no-op loop.
        core::hint::spin_loop();
    }
}

#[cfg(not(test))]
#[unsafe(no_mangle)]
/// # Safety
///
/// Secondary-hart S-mode entry: must run with `tp` = hartid < MAX_HARTS on
/// a hart whose private stack was set up by `secondary_continue`, with the
/// kernel root page table already loaded.
pub unsafe extern "Rust" fn secondary_kmain() -> ! {
    // SAFETY: tp holds a hartid < MAX_HARTS (set by entry asm / secondary_continue); G_ONLINE_HARTS is an aligned AtomicU32 static; sched_enter_idle never returns.
    unsafe {
        let hartid: usize;
        core::arch::asm!("mv {0}, tp", out(reg) hartid);
        crate::proc::process::set_cpu_online(hartid, true);
        G_ONLINE_HARTS.fetch_add(1, Ordering::SeqCst);
        crate::proc::scheduler::sched_enter_idle()
    }
}

#[cfg(test)]
#[unsafe(no_mangle)]
/// # Safety
///
/// Test stub: never runs; the crate is not booted under `cargo test`.
pub unsafe extern "Rust" fn secondary_kmain() -> ! {
    loop {
        // Divergence point: spin (never returns) without a busy no-op loop.
        core::hint::spin_loop();
    }
}
