//! Syscall subsystem.
//!
//! `abi`     — syscall numbers and ABI constants.
//! `handler` — the central `handle()` entry point, ACL, and `user_ptr_ok`.
//! `fs_sys`  — filesystem syscalls (`sys_write`, `sys_open`, …).
//! `fs`      — filesystem syscalls part 2 (`sys_exec`, `sys_sbrk`, …) and tty.
//! `fs_sys3` — filesystem syscalls part 3 (files, memory, time, signals).
//! `net`     — TCP/UDP syscalls (client, server, privileged configuration).
//! `proc`    — process, IPC, and input-event syscalls.
//! `misc`    — poll(2) readiness, ring transitions, and snapshots.
//! `power`   — power syscalls (`sys_reboot`: SBI SRST / QEMU finisher).
//!
//! The flat module paths of the old layout are re-exported, so
//! `crate::syscall::proc_sys`, `::net_sys`, `::tty`, … keep working.

pub mod abi;
pub mod fs_sys;
pub mod fs_sys3;
pub mod handler;

mod fs;
mod misc;
mod net;
mod power;
mod proc;

#[cfg(test)]
mod tests;

pub use abi::*;
pub use fs::{fs_sys2, tty};
pub use misc::{poll_sys, ring_sys, snap_sys};
pub use net::{net_admin_sys, net_server_sys, net_sys};
pub use power::power_sys;
pub use proc::{input_sys, ipc_sys, proc_sys};
