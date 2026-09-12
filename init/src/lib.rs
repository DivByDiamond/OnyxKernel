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
#![no_std]

pub mod syscalls;
pub mod term;
