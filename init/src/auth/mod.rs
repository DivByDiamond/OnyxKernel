// TODO(dead-code): auth::mod — shared auth/syscalls module, compiled per onyx_init bin;
// items unused by one binary are used by others (dead_code/unused_imports fire per-bin).
#![allow(dead_code, unused_imports)]

pub mod crypto;
pub mod group;
pub mod passwd;
pub mod shadow;

// Linker contract: onyx_core (shared crypto) declares `extern crate alloc`,
// so every binary compiling this module needs a global allocator. See
// kalloc.rs — inert unless something actually allocates.
mod kalloc;

pub const PASSWD_PATH: &[u8] = b"/etc/passwd";
pub const SHADOW_PATH: &[u8] = b"/etc/shadow";
pub const GROUP_PATH: &[u8] = b"/etc/group";
pub const MAX_USERS: usize = 16;
pub const MAX_GROUPS: usize = 16;
pub const MAX_LINE: usize = 256;

pub use crypto::*;
pub use group::*;
pub use passwd::*;
pub use shadow::*;

pub(crate) use group::atomic_rewrite;
pub(crate) use passwd::format_passwd_entry;
pub(crate) use shadow::format_shadow_entry;
