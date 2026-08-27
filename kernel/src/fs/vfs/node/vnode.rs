pub const VFS_MAX_FDS: usize = 16;
pub const MAX_MOUNTS: usize = 6;

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
pub const PERM_EXEC: u32 = 8;
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
        }
    }
}

pub type FdToken = u64;
pub const FD_TOKEN_NONE: FdToken = 0xFFFF_FFFF_FFFF_FFFF;

#[inline]
pub const fn fd_token(idx: usize, epoch: u32) -> FdToken {
    ((idx as u64) << 32) | (epoch as u64)
}

#[inline]
pub const fn fd_token_idx(token: FdToken) -> usize {
    (token >> 32) as usize
}

#[inline]
pub const fn fd_token_epoch(token: FdToken) -> u32 {
    token as u32
}
