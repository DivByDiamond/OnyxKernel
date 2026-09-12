//! Human-readable (POSIX macro name) rendering of `Errno`.

use super::Errno;

impl Errno {
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
