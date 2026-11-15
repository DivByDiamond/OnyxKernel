//! Shared allocator-free authentication, syscall and Lua VM APIs.
//!
//! Each executable explicitly installs its own allocator. Target-only I/O
//! stays behind the RISC-V gate; the VM and login policy also build on hosts.
#![cfg_attr(not(test), no_std)]

extern crate alloc;

#[cfg(target_arch = "riscv64")]
pub mod auth;
#[cfg(target_arch = "riscv64")]
pub mod syscalls;
#[cfg(target_arch = "riscv64")]
pub mod term;

pub mod luavm;

#[path = "login/password.rs"]
pub mod login_password;
