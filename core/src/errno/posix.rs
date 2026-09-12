//! POSIX/glibc `errno` numbering translation for the syscall boundary.

use super::Errno;

impl Errno {
    /// Translates a raw syscall return value from the internal `Errno`
    /// ordinal space to the POSIX-numbered `errno` userspace expects (see
    /// `to_posix`). Non-error values (`>= 0`) and anything that isn't a
    /// recognized internal error ordinal pass through unchanged — this is
    /// meant to wrap the single return point in the syscall dispatcher, not
    /// arbitrary data.
    #[inline]
    pub const fn translate_syscall_result(raw: i64) -> i64 {
        if raw >= 0 {
            return raw;
        }
        match Self::from_i64(raw) {
            Some(e) => -e.to_posix(),
            None => raw,
        }
    }

    /// Maps the compact internal ordinal (`Ok=0, NoMem=-1, Inval=-2, ...`)
    /// to the POSIX/glibc-numbered `errno` value userspace expects
    /// (`libonyxc/include/io/errno.h`, which documents this translation as
    /// already happening at the syscall boundary — this is that boundary).
    /// `as_i64()` alone is NOT what userspace should see: it would hand a
    /// C program `errno == 3` for what libonyxc calls `ENOENT` (2), etc.
    #[inline]
    pub const fn to_posix(self) -> i64 {
        match self {
            Self::Ok => 0,
            Self::Perm => 1,      // EPERM
            Self::NoEnt => 2,     // ENOENT
            Self::Io => 5,        // EIO
            Self::BadFd => 9,     // EBADF
            Self::Child => 10,    // ECHILD
            Self::Again => 11,    // EAGAIN
            Self::NoMem => 12,    // ENOMEM
            Self::Busy => 16,     // EBUSY
            Self::Exist => 17,    // EEXIST
            Self::NotDir => 20,   // ENOTDIR
            Self::IsDir => 21,    // EISDIR
            Self::Inval => 22,    // EINVAL
            Self::NoSpace => 28,  // ENOSPC
            Self::Pipe => 32,     // EPIPE
            Self::Range => 34,    // ERANGE
            Self::NoSys => 38,    // ENOSYS
            Self::NotEmpty => 39, // ENOTEMPTY
            Self::Loop => 40,     // ELOOP
            Self::Overflow => 75, // EOVERFLOW
            Self::Fault => 14,    // EFAULT
        }
    }
}
