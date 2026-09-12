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
#![allow(
    non_upper_case_globals,
    reason = "SYS_* constants (abi.rs) mirror Linux/kernel ABI syscall-name casing and are matched by that casing across dispatch/ACL sites (e.g. syscall/handler/acl.rs); module-level allows on abi.rs alone don't cover those use sites"
)]
#![allow(
    static_mut_refs,
    reason = "bare-static register/blocking state is pervasive in this no_std kernel; per-site migration to safe wrappers is tracked separately, not a stub"
)]
#![allow(
    clippy::too_many_arguments,
    reason = "syscall dispatch & trap-frame APIs mirror fixed hardware register layouts, not something to refactor away"
)]
#![allow(
    clippy::type_complexity,
    reason = "syscall dispatch & trap-frame APIs mirror fixed hardware register layouts, not something to refactor away"
)]
#![allow(
    clippy::missing_safety_doc,
    reason = "TODO(2026-09-13): crate-wide backstop for older unsafe fns predating the per-fn `# Safety` convention now enforced on new code; burn down opportunistically"
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

#[unsafe(no_mangle)]
pub unsafe extern "Rust" fn kmain(hartid: usize, fdt_addr: usize) -> ! {
    // Called from early boot once per hart; invariants are established by boot.S.
    unsafe { crate::srv::main::kmain(hartid, fdt_addr) }
}

#[cfg(not(test))]
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    crate::srv::klog::panic_handler(info)
}
