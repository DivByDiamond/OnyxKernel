#![expect(dead_code)]

pub mod crypto;
pub mod group;
pub mod passwd;
pub mod shadow;

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
