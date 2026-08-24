//! # OnyxKernel — RISC-V 64 (rv64gc) OS with Root Space / User Space isolation
//!
//! Full port of SlipperKernel→OnyxKernel in Rust.
//! ~98% Rust, assembly via `global_asm!`.
//!
//! ## Rings
//! - 0 (kernel): S-mode, OnyxKernel + drivers
//! - 1 (root space): U-mode, /bin/init + /service/*.bin + /bin/login
//! - 2 (user space): U-mode, /bin/osh + user programs

#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
#![warn(clippy::all)]
#![deny(clippy::correctness)]
#![warn(clippy::suspicious, clippy::style, clippy::complexity, clippy::perf)]
// SYS_* constants and G_* globals deliberately mirror the Linux/ABI naming
// used across syscall tables, ACLs and match dispatch sites.
#![allow(non_upper_case_globals)]
// Kernel-wide deliberate exceptions:
// - static_mut_refs: bare-static register/blocking state is pervasive in no_std kernel code
// - too_many_arguments / type_complexity: syscall & trap-frame APIs mirror hardware layout
#![allow(
    static_mut_refs,
    clippy::too_many_arguments,
    clippy::type_complexity,
    clippy::missing_safety_doc,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::must_use_candidate,
    clippy::module_name_repetitions,
    clippy::similar_names,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss
)]

extern crate alloc;
extern crate onyx_core;

pub mod arch;
pub mod drivers;
pub mod font;
pub mod fs;
pub mod ipc;
pub mod libfdt;
pub mod mm;
pub mod module;
pub mod net;
pub mod proc;
pub mod srv;
pub mod sync;
pub mod syscall;

use core::panic::PanicInfo;

#[unsafe(no_mangle)]
pub unsafe extern "Rust" fn kmain(hartid: usize, fdt_addr: usize) -> ! {
    // Called from early boot once per hart; invariants are established by boot.S.
    unsafe { crate::srv::main::kmain(hartid, fdt_addr) }
}

#[cfg(not(test))]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    crate::srv::klog::panic_handler(info)
}
