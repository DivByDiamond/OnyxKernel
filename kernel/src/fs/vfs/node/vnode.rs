pub const VFS_MAX_FDS: usize = 16;
pub const MAX_MOUNTS: usize = 8;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Fs {
    None = 0,
    Onyx = 1,
    Fat32 = 2,
    Proc = 3,
    Ipc = 4,
    Devfs = 5,
}

pub const PERM_READ: u32 = 1;
pub const PERM_WRITE: u32 = 2;
pub const PERM_SEEK: u32 = 4;
#[cfg(test)]
pub const PERM_EXEC: u32 = 8;
#[cfg(test)]
pub const PERM_ALL: u32 = PERM_READ | PERM_WRITE | PERM_SEEK | PERM_EXEC;

#[derive(Clone, Copy)]
pub struct VfsFd {
    pub ino: u32,
    pub size: u32,
    pub pos: u32,
    pub fs: Fs,
    pub used: bool,
    pub perms: u32,
    pub epoch: u32,
    pub cloexec: bool,
    /// User open(2) status flags recorded at open time (O_ACCMODE plus
    /// O_NONBLOCK/O_APPEND/...). F_GETFL returns this; F_SETFL updates the
    /// settable subset (todo P1 #4). Zero for kernel-internal slots.
    pub flags: u32,
    /// Mount-table slot this fd lives in (`G_MOUNTS[mnt]`), or
    /// `mount::MNT_ROOT as u8` for the root filesystem. Fd-based dispatch
    /// activates this mount's filesystem context before touching the
    /// backend singletons (see `vfs::with_mount`).
    pub mnt: u8,
}

impl Default for VfsFd {
    fn default() -> Self {
        Self {
            ino: 0,
            size: 0,
            pos: 0,
            fs: Fs::None,
            used: false,
            perms: 0,
            epoch: 0,
            cloexec: false,
            flags: 0,
            mnt: crate::fs::vfs::mount::MNT_ROOT as u8,
        }
    }
}

pub type FdToken = u64;
pub const FD_TOKEN_NONE: FdToken = 0xFFFF_FFFF_FFFF_FFFF;

#[inline]
pub const fn fd_token(idx: usize, epoch: u32) -> FdToken {
    // Compact 32-bit token for libonyxc compat: low 4 bits idx (0-15),
    // next 12 bits epoch low bits, stays positive (<2^31) and survives
    // truncation to int in lib's open() wrapper (which casts i64->int).
    // Epoch high bits are ignored for 32-bit callers but still provide
    // reuse detection for the 4096 most recent epochs.
    ((epoch as u64 & 0xFFF) << 4) | (idx as u64 & 0xF)
}

#[inline]
pub const fn fd_token_idx(token: FdToken) -> usize {
    (token & 0xF) as usize
}

#[inline]
pub const fn fd_token_epoch(token: FdToken) -> u32 {
    ((token >> 4) & 0xFFF) as u32
}
