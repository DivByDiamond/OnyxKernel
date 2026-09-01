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

    #[inline]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "OK",
            Self::NoMem => "ENOMEM",
            Self::Inval => "EINVAL",
            Self::NoEnt => "ENOENT",
            Self::Io => "EIO",
            Self::Perm => "EPERM",
            Self::Range => "ERANGE",
            Self::NoSys => "ENOSYS",
            Self::Busy => "EBUSY",
            Self::NoSpace => "ENOSPC",
            Self::NotDir => "ENOTDIR",
            Self::IsDir => "EISDIR",
            Self::BadFd => "EBADF",
            Self::Exist => "EEXIST",
            Self::Pipe => "EPIPE",
            Self::Overflow => "EOVERFLOW",
            Self::Child => "ECHILD",
            Self::NotEmpty => "ENOTEMPTY",
            Self::Loop => "ELOOP",
            Self::Again => "EAGAIN",
            Self::Fault => "EFAULT",
        }
    }
}

pub type KResult<T> = core::result::Result<T, Errno>;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_as_i64() {
        assert_eq!(Errno::Ok.as_i64(), 0);
        assert_eq!(Errno::NoMem.as_i64(), -1);
        assert_eq!(Errno::Inval.as_i64(), -2);
        assert_eq!(Errno::BadFd.as_i64(), -12);
        assert_eq!(Errno::NotEmpty.as_i64(), -17);
    }
    #[test]
    fn test_as_str() {
        assert_eq!(Errno::Ok.as_str(), "OK");
        assert_eq!(Errno::NoMem.as_str(), "ENOMEM");
        assert_eq!(Errno::Inval.as_str(), "EINVAL");
        assert_eq!(Errno::Perm.as_str(), "EPERM");
        assert_eq!(Errno::BadFd.as_str(), "EBADF");
        assert_eq!(Errno::NotEmpty.as_str(), "ENOTEMPTY");
    }
    #[test]
    fn test_kresult_ok() {
        let r: KResult<i32> = Ok(42);
        assert!(r.is_ok());
        assert_eq!(r, Ok(42));
    }
    #[test]
    fn test_kresult_err() {
        let r: KResult<i32> = Err(Errno::NoMem);
        assert!(r.is_err());
        assert_eq!(r, Err(Errno::NoMem));
    }
    #[test]
    fn test_to_posix_matches_libonyxc_errno_h() {
        // libonyxc/include/io/errno.h numbering — must stay in sync by hand,
        // there's no shared source of truth across the C/Rust boundary.
        assert_eq!(Errno::Perm.to_posix(), 1); // EPERM
        assert_eq!(Errno::NoEnt.to_posix(), 2); // ENOENT
        assert_eq!(Errno::Io.to_posix(), 5); // EIO
        assert_eq!(Errno::BadFd.to_posix(), 9); // EBADF
        assert_eq!(Errno::Child.to_posix(), 10); // ECHILD
        assert_eq!(Errno::Again.to_posix(), 11); // EAGAIN
        assert_eq!(Errno::NoMem.to_posix(), 12); // ENOMEM
        assert_eq!(Errno::Fault.to_posix(), 14); // EFAULT
        assert_eq!(Errno::Busy.to_posix(), 16); // EBUSY
        assert_eq!(Errno::Exist.to_posix(), 17); // EEXIST
        assert_eq!(Errno::NotDir.to_posix(), 20); // ENOTDIR
        assert_eq!(Errno::IsDir.to_posix(), 21); // EISDIR
        assert_eq!(Errno::Inval.to_posix(), 22); // EINVAL
        assert_eq!(Errno::NoSpace.to_posix(), 28); // ENOSPC
        assert_eq!(Errno::Pipe.to_posix(), 32); // EPIPE
        assert_eq!(Errno::Range.to_posix(), 34); // ERANGE
        assert_eq!(Errno::NoSys.to_posix(), 38); // ENOSYS
        assert_eq!(Errno::NotEmpty.to_posix(), 39); // ENOTEMPTY
        assert_eq!(Errno::Loop.to_posix(), 40); // ELOOP
    }

    #[test]
    fn test_from_i64_round_trips_as_i64() {
        let all = [
            Errno::NoMem,
            Errno::Inval,
            Errno::NoEnt,
            Errno::Io,
            Errno::Perm,
            Errno::Range,
            Errno::NoSys,
            Errno::Busy,
            Errno::NoSpace,
            Errno::NotDir,
            Errno::IsDir,
            Errno::BadFd,
            Errno::Exist,
            Errno::Pipe,
            Errno::Overflow,
            Errno::Child,
            Errno::NotEmpty,
            Errno::Loop,
            Errno::Again,
            Errno::Fault,
        ];
        for e in all {
            assert_eq!(Errno::from_i64(e.as_i64()), Some(e));
        }
        assert_eq!(Errno::from_i64(0), None); // Ok is not an error ordinal
        assert_eq!(Errno::from_i64(-21), None); // outside the enum's range
    }

    #[test]
    fn test_translate_syscall_result() {
        // Success and positive byte counts pass through unchanged.
        assert_eq!(Errno::translate_syscall_result(0), 0);
        assert_eq!(Errno::translate_syscall_result(4096), 4096);
        // Internal ordinal -3 (NoEnt) becomes POSIX -2 (ENOENT), not -3.
        assert_eq!(Errno::translate_syscall_result(Errno::NoEnt.as_i64()), -2);
        assert_eq!(Errno::translate_syscall_result(Errno::Io.as_i64()), -5);
        assert_eq!(Errno::translate_syscall_result(Errno::Inval.as_i64()), -22);
    }

    #[test]
    fn test_all_errno_variants() {
        let variants = [
            (Errno::Ok, 0, "OK"),
            (Errno::NoMem, -1, "ENOMEM"),
            (Errno::Inval, -2, "EINVAL"),
            (Errno::NoEnt, -3, "ENOENT"),
            (Errno::Io, -4, "EIO"),
            (Errno::Perm, -5, "EPERM"),
            (Errno::Range, -6, "ERANGE"),
            (Errno::NoSys, -7, "ENOSYS"),
            (Errno::Busy, -8, "EBUSY"),
            (Errno::NoSpace, -9, "ENOSPC"),
            (Errno::NotDir, -10, "ENOTDIR"),
            (Errno::IsDir, -11, "EISDIR"),
            (Errno::BadFd, -12, "EBADF"),
            (Errno::Exist, -13, "EEXIST"),
            (Errno::Pipe, -14, "EPIPE"),
            (Errno::Overflow, -15, "EOVERFLOW"),
            (Errno::Child, -16, "ECHILD"),
            (Errno::NotEmpty, -17, "ENOTEMPTY"),
            (Errno::Loop, -18, "ELOOP"),
            (Errno::Again, -19, "EAGAIN"),
            (Errno::Fault, -20, "EFAULT"),
        ];
        for (e, code, name) in variants {
            assert_eq!(e.as_i64(), code, "{} code mismatch", name);
            assert_eq!(e.as_str(), name, "{} name mismatch", name);
        }
    }
}
