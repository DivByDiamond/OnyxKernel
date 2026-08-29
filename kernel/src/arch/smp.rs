use core::sync::atomic::{AtomicU32, Ordering};

// Boot-protocol word width: the OnyxBoot SMP mailbox and the G_RELEASE
// spin flag hold one machine word. rv32imac lacks 64-bit atomics (no
// A-extension 64-bit AMOs), so the protocol degrades to AtomicU32 there.
mod secondary;
#[cfg(target_pointer_width = "64")]
type MailboxAtomic = core::sync::atomic::AtomicU64;
#[cfg(target_pointer_width = "32")]
type MailboxAtomic = core::sync::atomic::AtomicU32;
#[cfg(target_pointer_width = "64")]
type MailboxVal = u64;
#[cfg(target_pointer_width = "32")]
type MailboxVal = u32;

pub const MAX_HARTS: usize = 8;
pub const SEC_STACK_SIZE: usize = 4096;

#[unsafe(no_mangle)]
pub static mut G_SEC_STACKS: [u8; MAX_HARTS * SEC_STACK_SIZE] = [0; MAX_HARTS * SEC_STACK_SIZE];

// Bug (syscall SERIOUS #10): make G_ONLINE_HARTS atomic so concurrent
// secondary hart bring-up doesn't race on the increment.
pub(super) static G_ONLINE_HARTS: AtomicU32 = AtomicU32::new(1);

#[cfg(target_pointer_width = "64")]
#[unsafe(no_mangle)]
pub static mut G_RELEASE: u64 = 0;
#[cfg(target_pointer_width = "32")]
#[unsafe(no_mangle)]
pub static mut G_RELEASE: u32 = 0;

#[unsafe(no_mangle)]
pub static mut G_KERNEL_ROOT_PA: u64 = 0;

pub fn current_hart() -> usize {
    #[cfg(not(test))]
    {
        let hartid: usize;
        // SAFETY: reads tp, which the boot asm sets to this hart's hartid on every hart.
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
    // SAFETY: caller guarantees hart < MAX_HARTS, so the G_CPU_ONLINE index is in bounds.
    unsafe { (G_CPU_ONLINE)[hart] }
}

/// Mark a hart online in the `G_CPU_ONLINE` table.
///
/// # Safety
///
/// Caller guarantees `hart < MAX_HARTS`; must not race with concurrent
/// writers of the same slot (bring-up is serialized by the boot protocol).
pub unsafe fn set_cpu_online(hart: usize, v: bool) {
    // SAFETY: caller guarantees hart < MAX_HARTS, so the G_CPU_ONLINE index is in bounds; writers are serialized by the boot protocol.
    unsafe {
        G_CPU_ONLINE[hart] = v;
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
fn mailbox() -> &'static MailboxAtomic {
    // SAFETY: 0x80100000 is a naturally-aligned DRAM word reserved by the
    // boot protocol (see SMP_MAILBOX_PA); it stays mapped and valid forever.
    unsafe { MailboxAtomic::from_ptr(SMP_MAILBOX_PA as *mut MailboxVal) }
}

/// Release every parked secondary hart (bootloader mailbox + G_RELEASE).
///
/// # Safety
///
/// Must be called exactly once, from hart 0, after kernel initialization
/// the secondaries rely on (root page table, heap, scheduler) is complete;
/// never while holding a spinlock (secondaries immediately run kernel code).
pub unsafe fn release_secondary_harts() {
    // Two release channels for two boot topologies:
    //   1. Bootloader mailbox (OnyxBoot): parked harts poll SMP_MAILBOX_PA
    //      and jump straight to `secondary_continue` when it becomes
    //      non-zero. SeqCst store + their fence guarantees all prior kernel
    //      initialization is visible before they jump.
    //   2. G_RELEASE flag: firmwares that hand EVERY hart to the kernel
    //      entry (-bios none / OpenSBI-style) leave them spinning inside
    //      `secondary_entry`; setting the flag releases those.
    let entry = secondary::secondary_continue as *const () as usize as MailboxVal;
    mailbox().store(entry, Ordering::SeqCst);
    // SAFETY: G_RELEASE is a naturally-aligned machine-word static that
    // outlives the program; see the matching pattern in `mailbox()` above.
    unsafe {
        MailboxAtomic::from_ptr(core::ptr::addr_of_mut!(G_RELEASE)).store(1, Ordering::SeqCst);
    }
}

#[cfg(not(test))]
#[unsafe(no_mangle)]
/// # Safety
///
/// Firmware-side secondary entry: every hart lands here in M-mode with
/// `tp` = hartid < MAX_HARTS (boot protocol); spins on `G_RELEASE`, then
/// jumps to `secondary::secondary_continue` and never returns.
pub unsafe extern "Rust" fn secondary_entry() -> ! {
    // SAFETY: G_RELEASE is an aligned machine-word static that outlives the program (same contract as the store in release_secondary_harts); the asm is a bare wfi that only sleeps.
    unsafe {
        loop {
            // Firmware paths that hand EVERY hart to the kernel entry
            // (-bios none / OpenSBI-style) wait here on G_RELEASE instead of
            // the bootloader mailbox. Acquire load pairs with the SeqCst
            // store in release_secondary_harts... which now also publishes
            // `secondary_continue` into the mailbox for OnyxBoot-parked
            // harts; setting the flag here wakes those waiters too.
            if MailboxAtomic::from_ptr(core::ptr::addr_of_mut!(G_RELEASE))
                .load(core::sync::atomic::Ordering::Acquire)
                != 0
            {
                break;
            }
            core::arch::asm!("wfi");
        }
        secondary::secondary_continue()
    }
}

pub fn online_harts() -> u32 {
    G_ONLINE_HARTS.load(Ordering::SeqCst)
}
