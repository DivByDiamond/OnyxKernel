use crate::sync::SpinLock;

pub mod devfs;
pub mod fat32;
pub mod ipcfs;
pub mod onyxfs;
pub mod procfs;
pub mod pty;
pub mod vfs;

/// Serializes every operation that mutates or reads the block-filesystem
/// singleton state (`onyxfs`/`fat32` module globals). Path-based dispatch
/// switches the singletons to the target mount for the duration of one
/// operation (`vfs::with_mount`); this lock makes that switch atomic across
/// harts. Taken only in trap/syscall/boot context with SIE=0, matching the
/// `SpinLock` interrupt invariant (see `crate::sync`).
pub(crate) static FS_LOCK: SpinLock = SpinLock::new();

/// Linux-compatible `dirent.d_type` values returned by the readdir backends
/// and written into getdents64 records. Only the four kinds OnyxFS devices
/// and directories can currently be are defined; symlink/socket support
/// adds its own here.
pub const DT_DIR: u8 = 4;
pub const DT_REG: u8 = 8;
pub const DT_CHR: u8 = 2;
pub const DT_BLK: u8 = 6;

#[cfg(test)]
mod tests;
