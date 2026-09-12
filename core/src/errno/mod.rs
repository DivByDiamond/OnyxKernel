mod display;
mod posix;
#[cfg(test)]
mod tests;

#[repr(i64)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Errno {
    Ok = 0,
    NoMem = -1,
    Inval = -2,
    NoEnt = -3,
    Io = -4,
    Perm = -5,
    Range = -6,
    NoSys = -7,
    Busy = -8,
    NoSpace = -9,
    NotDir = -10,
    IsDir = -11,
    BadFd = -12,
    Exist = -13,
    Pipe = -14,
    Overflow = -15,
    Child = -16,
    NotEmpty = -17,
    Loop = -18,
    /// EAGAIN — resource exhaustion (e.g. process-count limit hit by
    /// fork/spawn); caller should retry later, like POSIX fork(2).
    Again = -19,
    /// EFAULT — bad user-space pointer: unmapped page or missing PTE_U in
    /// a buffer passed to a syscall. Returned instead of faulting in S-mode.
    Fault = -20,
}

impl Errno {
    #[inline]
    pub const fn as_i64(self) -> i64 {
        self as i64
    }

    /// Reconstructs the `Errno` a syscall handler returned from its raw
    /// `as_i64()` value. Returns `None` for `0` (success, not an error) or
    /// anything outside the enum's `-1..=-20` range.
    #[inline]
    pub const fn from_i64(code: i64) -> Option<Self> {
        Some(match code {
            -1 => Self::NoMem,
            -2 => Self::Inval,
            -3 => Self::NoEnt,
            -4 => Self::Io,
            -5 => Self::Perm,
            -6 => Self::Range,
            -7 => Self::NoSys,
            -8 => Self::Busy,
            -9 => Self::NoSpace,
            -10 => Self::NotDir,
            -11 => Self::IsDir,
            -12 => Self::BadFd,
            -13 => Self::Exist,
            -14 => Self::Pipe,
            -15 => Self::Overflow,
            -16 => Self::Child,
            -17 => Self::NotEmpty,
            -18 => Self::Loop,
            -19 => Self::Again,
            -20 => Self::Fault,
            _ => return None,
        })
    }
}

pub type KResult<T> = core::result::Result<T, Errno>;
