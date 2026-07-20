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
}

impl Errno {
    #[inline]
    pub const fn as_i64(self) -> i64 {
        self as i64
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
        assert_eq!(r.unwrap(), 42);
    }
    #[test]
    fn test_kresult_err() {
        let r: KResult<i32> = Err(Errno::NoMem);
        assert!(r.is_err());
        assert_eq!(r.unwrap_err(), Errno::NoMem);
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
        ];
        for (e, code, name) in variants {
            assert_eq!(e.as_i64(), code, "{} code mismatch", name);
            assert_eq!(e.as_str(), name, "{} name mismatch", name);
        }
    }
}
