//! Assembly: boot.S + trap.S via global_asm!.
//! KEY FIXES vs C-version:
//! - PMP: single 4GB region (pmpaddr0=0x3FFFFFFF, pmpcfg0=0x9F)
//! - trap_entry does NOT switch satp (keeps user satp + SUM bit)
//! - drop_to_user does NOT zero gp/tp
//! - sscratch initialized to __stack_top in trap::init
#[cfg(all(not(test), target_pointer_width = "64"))]
pub mod boot;
#[cfg(all(not(test), target_pointer_width = "32"))]
pub mod boot_32;
#[cfg(all(not(test), target_pointer_width = "64"))]
pub mod trap_asm;
#[cfg(all(not(test), target_pointer_width = "32"))]
pub mod trap_asm_32;

#[cfg(all(not(test), target_pointer_width = "64"))]
pub use trap_asm::{drop_to_user, sched_switch, trap_entry, trap_return};
#[cfg(all(not(test), target_pointer_width = "32"))]
pub use trap_asm_32::{drop_to_user, sched_switch, trap_entry, trap_return};

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn trap_handler(tf: *mut crate::arch::trap_frame::TrapFrame) {
    let frame = unsafe { &mut *tf };
    unsafe {
        crate::srv::trap::handle(frame);
    }
}

#[cfg(test)]
pub unsafe fn trap_entry() {}
#[cfg(test)]
pub unsafe fn trap_return() {}
#[cfg(test)]
pub unsafe fn sched_switch(_new_sp: usize) -> ! {
    loop {}
}
#[cfg(test)]
pub unsafe fn drop_to_user(_entry: usize, _ustack: usize, _user_root_pa: usize) -> ! {
    loop {}
}
