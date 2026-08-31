//! Assembly: boot.S + trap.S via global_asm!.
//! KEY FIXES vs C-version:
//! - PMP: single 4GB region (pmpaddr0=0x3FFFFFFF, pmpcfg0=0x9F)
//! - trap_entry does NOT switch satp (keeps user satp + SUM bit)
//! - drop_to_user does NOT zero gp/tp
//! - sscratch initialized to __stack_top in trap::init
#[cfg(all(not(test), target_pointer_width = "64", not(feature = "smode")))]
pub mod boot;
#[cfg(all(not(test), target_pointer_width = "32", not(feature = "smode")))]
pub mod boot_32;
#[cfg(all(not(test), target_pointer_width = "64", feature = "smode"))]
pub mod boot_smode;
#[cfg(all(not(test), target_pointer_width = "64"))]
pub mod trap_asm;
#[cfg(all(not(test), target_pointer_width = "32"))]
pub mod trap_asm_32;

#[cfg(all(not(test), target_pointer_width = "64"))]
pub use trap_asm::{drop_to_user, sched_switch, trap_entry, trap_return};
#[cfg(all(not(test), target_pointer_width = "32"))]
pub use trap_asm_32::{drop_to_user, sched_switch, trap_entry, trap_return};

/// # Safety
/// `tf` must point to a valid, exclusively-owned trap frame pushed by `trap_entry` in trap.S.
#[cfg(not(test))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trap_handler(tf: *mut crate::arch::trap_frame::TrapFrame) {
    // SAFETY: per contract, `tf` is the live frame for this hart and is not
    // aliased by any other hart, so a single `&mut` is sound.
    let frame = unsafe { &mut *tf };
    // SAFETY: `frame` satisfies `handle`'s requirement of an exclusively-
    // owned live trap frame; we run in S-mode with SIE cleared (hardware).
    unsafe {
        crate::srv::trap::handle(frame);
    }
}

// SAFETY (test stubs): these are no-ops so host-side unit tests can link;
// they perform no memory access and uphold no invariants.
#[cfg(test)]
/// # Safety
/// Test stub: no-op, safe to call with any arguments.
pub unsafe fn trap_entry() {}
#[cfg(test)]
/// # Safety
/// Test stub: never returns.
pub unsafe fn trap_return() {}
#[cfg(test)]
/// # Safety
/// Test stub: never returns; `_new_sp` unused.
pub unsafe fn sched_switch(_new_sp: usize) -> ! {
    loop {
        // Divergence point: spin (never returns) without a busy no-op loop.
        core::hint::spin_loop();
    }
}
#[cfg(test)]
/// # Safety
/// Test stub: never returns; arguments unused.
pub unsafe fn drop_to_user(_entry: usize, _ustack: usize, _user_root_pa: usize) -> ! {
    loop {
        // Divergence point: spin (never returns) without a busy no-op loop.
        core::hint::spin_loop();
    }
}
