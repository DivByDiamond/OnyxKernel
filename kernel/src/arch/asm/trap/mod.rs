//! S-mode trap entry/return, `sched_switch`, and `drop_to_user`.
//!
//! [`trap_asm`] holds the rv64 trap vector, [`trap_asm_32`] the rv32 one.

#[cfg(target_pointer_width = "64")]
mod trap_asm;
#[cfg(target_pointer_width = "32")]
mod trap_asm_32;

#[cfg(target_pointer_width = "64")]
pub use trap_asm::{drop_to_user, sched_switch, trap_entry, trap_return};
#[cfg(target_pointer_width = "32")]
pub use trap_asm_32::{drop_to_user, sched_switch, trap_entry, trap_return};
