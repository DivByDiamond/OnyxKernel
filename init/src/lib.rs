//! Shared library surface for the `onyx_init` binaries.
//!
//! `syscalls`/`term` used to be `mod`-included separately into each of the
//! 14 bin crates, so per-bin unused items triggered `dead_code`/
//! `unused_imports` warnings that could only be silenced with a blanket
//! `#[allow]` (see git history). Moving them here means they're compiled
//! once as this crate's public API: unused-from-a-given-bin items are
//! still part of the crate's public surface, not dead code.
//!
//! `auth` deliberately stays out of this lib and is still `mod`-included
//! per bin: it pulls in `auth::kalloc`'s `#[global_allocator]`, and
//! `onyx-lua` has its own allocator (`luavm::kalloc`) — one shared lib used
//! by *all* 14 bins uniformly can only carry allocator-free modules,
//! otherwise every bin (even ones that never touch `auth`) would drag that
//! allocator into their link and `onyx-lua` would hard-fail on "duplicate
//! `#[global_allocator]`" (verified: this is exactly what happened when
//! `auth` was included here).
// `no_std` for every real build; host test builds link std so the libtest
// harness gets a normal runtime (a `no_std` test binary segfaults in startup).
#![cfg_attr(not(test), no_std)]

// The syscall layer is raw RISC-V `ecall` asm and the terminal helpers call
// it, so neither builds for an x86_64 host. Host test runs (`cargo test
// -p onyx_init --lib`) exclude them and exercise only the pure-logic modules
// bridged below; the real target always gets both.
#[cfg(any(target_arch = "riscv64", not(test)))]
pub mod syscalls;
#[cfg(any(target_arch = "riscv64", not(test)))]
pub mod term;

// Host test bridge: the tested modules live in `onyx-*` binaries (which pull
// in asm), but the code under test is pure. Including the sources here lets
// `cargo test -p onyx_init --lib` run them on the host with the module paths
// they already use (`crate::luavm::value`, `super::value`).
#[cfg(test)]
#[macro_use]
extern crate alloc;

#[cfg(test)]
pub mod luavm {
    #[path = "value.rs"]
    pub mod value;

    #[path = "vm/mod.rs"]
    pub mod vm;
}

#[cfg(test)]
#[path = "login/password.rs"]
mod login_password;
