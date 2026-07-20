use super::{Fs, resolve_mount};
use crate::fs::onyxfs;
use onyx_core::errno::{Errno, KResult};
use onyx_core::formats::ONYFS_ROOT_INO;

unsafe fn split_parent(path: &[u8]) -> (&[u8], &[u8]) {
    let p = if !path.is_empty() && path[0] == b'/' {
        &path[1..]
    } else {
        path
    };
    match p.iter().rposition(|&b| b == b'/') {
        Some(idx) => (&p[..idx], &p[idx + 1..]),
        None => (&[], p),
    }
}

pub unsafe fn symlink(target: &[u8], linkpath: &[u8]) -> KResult<()> {
    if linkpath.is_empty() || linkpath[0] != b'/' {
        return Err(Errno::Inval);
    }
    let name = &linkpath[1..];
    let (fs, _) = resolve_mount(name);
    if fs != Fs::Onyx {
        return Err(Errno::NoSys);
    }
    let (parent_path, filename) = split_parent(linkpath);
    if filename.is_empty() {
        return Err(Errno::Inval);
    }
    let mut st = onyxfs::OnyfsStat::default();
    let parent_ino = if parent_path.is_empty() {
        ONYFS_ROOT_INO
    } else {
        onyxfs::lookup(parent_path, &mut st)?
    };
    onyxfs::symlink(parent_ino, filename, target)?;
    Ok(())
}

pub unsafe fn readlink(path: &[u8], buf: *mut u8, bufsiz: u32) -> KResult<u32> {
    if path.is_empty() || path[0] != b'/' {
        return Err(Errno::Inval);
    }
    let name = &path[1..];
    let (fs, _) = resolve_mount(name);
    if fs != Fs::Onyx {
        return Err(Errno::NoSys);
    }
    let mut st = onyxfs::OnyfsStat::default();
    let ino = onyxfs::lookup(name, &mut st)?;
    onyxfs::readlink(ino, buf, bufsiz)
}
