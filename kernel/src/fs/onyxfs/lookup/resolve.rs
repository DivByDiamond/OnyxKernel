use super::super::inode::{read_inode, stat};
use super::super::{G_BUF, OnyfsStat, dirents_per_block, read_block};
use super::parse_dirent;
use onyx_core::errno::{Errno, KResult};
use onyx_core::formats::{
    ONYFS_DIRECT_BLKS, ONYFS_DT_DIR, ONYFS_NAME_MAX, ONYFS_ROOT_INO, OnyfsInode,
};

/// # Safety
///
/// Caller must not invoke onyxfs operations concurrently from multiple harts
/// (shared G_BUF scratch global). `dir_ino` must be a valid directory inode;
/// `name` is a kernel-owned slice compared in-bounds against dirent names.
pub unsafe fn lookup_in(dir_ino: u32, name: &[u8], out: &mut OnyfsStat) -> KResult<u32> {
    // SAFETY: single-threaded onyxfs exclusion (see # Safety); dirent slots
    // are bounds-checked inside parse_dirent before each G_BUF read.
    unsafe {
        let mut inode = OnyfsInode {
            mode: 0,
            size: 0,
            uid: 0,
            gid: 0,
            nlink: 0,
            blocks: [0; ONYFS_DIRECT_BLKS],
            indirect: 0,
            double_indirect: 0,
            crtime: 0,
            mtime: 0,
            atime: 0,
            ctime: 0,
            flags: 0,
            reserved: 0,
        };
        read_inode(dir_ino, &mut inode)?;
        let dpb = dirents_per_block();
        for blk_idx in 0..ONYFS_DIRECT_BLKS {
            let dir_blk = inode.blocks[blk_idx];
            if dir_blk == 0 {
                continue;
            }
            {
                let pb = &raw mut G_BUF;
                read_block(dir_blk, &mut *pb)
            }?;
            for i in 0..dpb {
                let d = parse_dirent(i)?;
                if d.inode == 0 {
                    continue;
                }
                let nl = if d.name_len > 0 && (d.name_len as usize) <= ONYFS_NAME_MAX {
                    d.name_len as usize
                } else {
                    d.name
                        .iter()
                        .position(|&b| b == 0)
                        .unwrap_or(ONYFS_NAME_MAX)
                };
                if nl == name.len() && d.name[..nl] == *name {
                    let found_ino = d.inode;
                    stat(found_ino, out)?;
                    return Ok(found_ino);
                }
            }
        }
        Err(Errno::NoEnt)
    }
}

/// Resolve `path` from the OnyxFS root, following symlinks encountered on
/// the way (recursion capped at depth 8 → ELOOP). This is the lookup entry
/// point used by open/stat/chmod/chown/utimens; wiring symlink resolution
/// here closes the contract gap where symlinks could be created
/// (sys_symlink) but were never resolved during path traversal.
///
/// # Safety
///
/// Caller must not invoke onyxfs operations concurrently from multiple harts.
/// `path` must be a kernel-owned byte string (syscall layer parses user
/// paths first); `out` is a caller-owned writable OnyfsStat.
pub unsafe fn lookup(path: &[u8], out: &mut OnyfsStat) -> KResult<u32> {
    // SAFETY: delegates to lookup_follow, whose contract covers all raw access.
    unsafe { super::follow::lookup_follow(path, out, 0) }
}

/// Resolve `path` WITHOUT following a symlink at the final component
/// (POSIX lstat/readlink/unlink/rename semantics). Used by operations that
/// must act on the link itself rather than its target.
///
/// # Safety
///
/// Same contract as `lookup`: single-threaded onyxfs exclusion; `path` is a
/// kernel-owned slice (user paths parsed by the syscall layer).
pub unsafe fn lookup_nofollow(path: &[u8], out: &mut OnyfsStat) -> KResult<u32> {
    // SAFETY: only slices the caller-owned `path` in-bounds and delegates
    // raw block access to the bounds-checked lookup_in/stat helpers.
    unsafe {
        let mut cur_ino = ONYFS_ROOT_INO;
        let mut remaining = path;
        loop {
            while !remaining.is_empty() && remaining[0] == b'/' {
                remaining = &remaining[1..];
            }
            if remaining.is_empty() {
                break;
            }
            let component = match remaining.iter().position(|&b| b == b'/') {
                Some(idx) => &remaining[..idx],
                None => remaining,
            };
            if component.is_empty() {
                break;
            }
            let mut tmp = OnyfsStat::default();
            cur_ino = lookup_in(cur_ino, component, &mut tmp)?;
            match remaining.iter().position(|&b| b == b'/') {
                Some(idx) => remaining = &remaining[idx + 1..],
                None => break,
            }
        }
        stat(cur_ino, out)?;
        Ok(cur_ino)
    }
}

/// # Safety
///
/// Same contract as `lookup`: single-threaded onyxfs exclusion; `path` is a
/// kernel-owned slice (user paths parsed by the syscall layer).
pub unsafe fn resolve_dir(path: &[u8]) -> KResult<u32> {
    // SAFETY: delegates to lookup, whose contract covers all raw access.
    unsafe {
        let mut st = OnyfsStat::default();
        let ino = lookup(path, &mut st)?;
        if st.mode & 0o170000 != ONYFS_DT_DIR & 0o170000 {
            return Err(Errno::NotDir);
        }
        Ok(ino)
    }
}
