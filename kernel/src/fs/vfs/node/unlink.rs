use crate::fs::onyxfs;
use crate::fs::vfs::resolve_mount;
use onyx_core::errno::{Errno, KResult};

pub unsafe fn unlink(path: &[u8]) -> KResult<()> {
    unsafe {
        if path.is_empty() || path[0] != b'/' {
            return Err(Errno::Inval);
        }
        let name = &path[1..];
        let (fs, _) = resolve_mount(name);
        if fs != crate::fs::vfs::Fs::Onyx {
            return Err(Errno::NoSys);
        }
        onyxfs::unlink(path)
    }
}
