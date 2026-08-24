use core::hint::spin_loop;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

pub const MAX_HARTS: usize = 8;
pub const SEC_STACK_SIZE: usize = 4096;

#[unsafe(no_mangle)]
pub static mut G_SEC_STACKS: [u8; MAX_HARTS * SEC_STACK_SIZE] = [0; MAX_HARTS * SEC_STACK_SIZE];

// Bug (syscall SERIOUS #10): make G_ONLINE_HARTS atomic so concurrent
// secondary hart bring-up doesn't race on the increment.
static G_ONLINE_HARTS: AtomicU32 = AtomicU32::new(1);

#[unsafe(no_mangle)]
pub static mut G_RELEASE: u64 = 0;

#[unsafe(no_mangle)]
pub static mut G_KERNEL_ROOT_PA: u64 = 0;

pub fn current_hart() -> usize {
    #[cfg(not(test))]
    {
        let hartid: usize;
        unsafe {
            core::arch::asm!("mv {}, tp", out(reg) hartid);
        }
        hartid
    }
    #[cfg(test)]
    {
        0
    }
}

static mut G_CPU_ONLINE: [bool; MAX_HARTS] =
    [true, false, false, false, false, false, false, false];

pub fn cpu_online(hart: usize) -> bool {
    unsafe { (G_CPU_ONLINE)[hart] }
}

pub unsafe fn set_cpu_online(hart: usize, v: bool) {
    unsafe {
        G_CPU_ONLINE[hart] = v;
    }
}

pub struct SpinLock {
    locked: AtomicBool,
}

impl Default for SpinLock {
    fn default() -> Self {
        Self::new()
    }
}

impl SpinLock {
    pub const fn new() -> Self {
        SpinLock {
            locked: AtomicBool::new(false),
        }
    }
    pub fn lock(&self) {
        while self.locked.swap(true, Ordering::Acquire) {
            while self.locked.load(Ordering::Relaxed) {
                spin_loop();
            }
        }
    }
    pub fn unlock(&self) {
        self.locked.store(false, Ordering::Release);
    }
}

/// Fixed SMP release mailbox, shared with the bootloader.
///
/// OnyxBoot parks secondary harts polling this physical address (see
/// OnyxBoot/src/boot_entry.c). It lives in the unused 2 MB gap between the
/// bootloader image @0x80000000 and the kernel @0x80200000, so neither side
/// can clobber it. The kernel publishes the S-mode secondary entry address
/// here once it is ready to run secondaries; parked harts jump straight to
/// it with tp = hartid. The kernel maps VA == PA for this region (identity
/// mapping), so the same constant works from both sides.
pub const SMP_MAILBOX_PA: usize = 0x8010_0000;

#[inline]
fn mailbox() -> &'static core::sync::atomic::AtomicU64 {
    // SAFETY: 0x80100000 is a naturally-aligned DRAM word reserved by the
    // boot protocol (see SMP_MAILBOX_PA); it stays mapped and valid forever.
    unsafe { core::sync::atomic::AtomicU64::from_ptr(SMP_MAILBOX_PA as *mut u64) }
}

pub unsafe fn release_secondary_harts() {
    // Two release channels for two boot topologies:
    //   1. Bootloader mailbox (OnyxBoot): parked harts poll SMP_MAILBOX_PA
    //      and jump straight to `secondary_continue` when it becomes
    //      non-zero. SeqCst store + their fence guarantees all prior kernel
    //      initialization is visible before they jump.
    //   2. G_RELEASE flag: firmwares that hand EVERY hart to the kernel
    //      entry (-bios none / OpenSBI-style) leave them spinning inside
    //      `secondary_entry`; setting the flag releases those.
    let entry = secondary_continue as *const () as usize as u64;
    mailbox().store(entry, Ordering::SeqCst);
    // SAFETY: G_RELEASE is a naturally-aligned u64 static that outlives the
    // program; see the matching pattern in `mailbox()` above.
    unsafe {
        core::sync::atomic::AtomicU64::from_ptr(core::ptr::addr_of_mut!(G_RELEASE))
            .store(1, Ordering::SeqCst);
    }
}

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub unsafe extern "Rust" fn secondary_entry() -> ! {
    unsafe {
        loop {
            // Firmware paths that hand EVERY hart to the kernel entry
            // (-bios none / OpenSBI-style) wait here on G_RELEASE instead of
            // the bootloader mailbox. Acquire load pairs with the SeqCst
            // store in release_secondary_harts... which now also publishes
            // `secondary_continue` into the mailbox for OnyxBoot-parked
            // harts; setting the flag here wakes those waiters too.
            if core::sync::atomic::AtomicU64::from_ptr(core::ptr::addr_of_mut!(G_RELEASE))
                .load(core::sync::atomic::Ordering::Acquire)
                != 0
            {
                break;
            }
            core::arch::asm!("wfi");
        }
        secondary_continue()
    }
}

/// Common continuation once a secondary hart is allowed to run: switch to
/// its private stack, load the kernel root page table and drop to S-mode in
/// `secondary_kmain`. Reached either from `secondary_entry` (firmware hands
/// every hart to the kernel) or directly via the bootloader SMP mailbox.
#[cfg(not(test))]
pub unsafe extern "Rust" fn secondary_continue() -> ! {
    unsafe {
        let hartid: usize;
        core::arch::asm!("mv {0}, tp", out(reg) hartid);
        let sp = &raw const G_SEC_STACKS as *const u8 as usize + (hartid + 1) * SEC_STACK_SIZE;
        let entry = secondary_kmain as *const () as usize;
        let root_pa = core::ptr::read_volatile(&raw const G_KERNEL_ROOT_PA);
        let satp = if root_pa != 0 {
            #[cfg(target_pointer_width = "64")]
            {
                (8u64 << 60) | (root_pa >> 12)
            }
            #[cfg(target_pointer_width = "32")]
            {
                (crate::arch::bits::SATP_MODE_SV32 as u64) | ((root_pa >> 12) & 0x3FFFFF)
            }
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
pub unsafe extern "Rust" fn secondary_entry() -> ! {
    loop {}
}

// Test twin of the real `secondary_continue`: `release_secondary_harts`
// publishes its address into the bootloader mailbox unconditionally, so the
// symbol must exist when the crate is built for `cargo test` (where the
// real M-mode/S-mode trampoline is compiled out).
#[cfg(test)]
pub unsafe extern "Rust" fn secondary_continue() -> ! {
    loop {}
}

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub unsafe extern "Rust" fn secondary_kmain() -> ! {
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
pub unsafe extern "Rust" fn secondary_kmain() -> ! {
    loop {}
}

pub fn online_harts() -> u32 {
    G_ONLINE_HARTS.load(Ordering::SeqCst)
}
