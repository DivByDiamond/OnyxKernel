//! Path resolution and dirent parsing.
//!
//! `lookup_in` resolves a single name within a directory; `lookup` walks a
//! slash-separated path starting from the root inode. `parse_dirent` handles
//! both v1 (36-byte) and v2 (40-byte) dirent layouts, returning the v2
//! `OnyfsDirent` struct in both cases. `resolve_dir` resolves a directory
//! path (returns ENOTDIR if the target is a regular file).
//!
//! `readdir_entry` (stateful directory iteration) lives in `readdir.rs`.
use super::inode::{read_inode, stat};
use super::{
    G_BUF, G_VERSION, ONYFS_V1, ONYFS_V1_DIRENT_SIZE, OnyfsStat, dirents_per_block, read_block,
};
use onyx_core::errno::{Errno, KResult};
use onyx_core::formats::{
    ONYFS_BLOCK_SIZE, ONYFS_DIRECT_BLKS, ONYFS_DT_DIR, ONYFS_NAME_MAX, ONYFS_ROOT_INO, OnyfsDirent,
    OnyfsInode,
};

/// Parse a dirent from the current `G_BUF` contents at the given slot index.
/// Handles both v1 (36-byte) and v2 (40-byte) layouts, returning the v2
/// `OnyfsDirent` struct in both cases.
pub(super) unsafe fn parse_dirent(slot: usize) -> KResult<OnyfsDirent> {
    let buf_view: &[u8] = &(*(&raw const G_BUF));
    match *(&raw const G_VERSION) {
        ONYFS_V1 => {
            let off = slot * ONYFS_V1_DIRENT_SIZE;
            if off + ONYFS_V1_DIRENT_SIZE > ONYFS_BLOCK_SIZE {
                return Err(Errno::Inval);
            }
            let s = &buf_view[off..off + ONYFS_V1_DIRENT_SIZE];
            let mut name = [0u8; ONYFS_NAME_MAX];
            name.copy_from_slice(&s[0..ONYFS_NAME_MAX]);
            let inode = u32::from_le_bytes([s[32], s[33], s[34], s[35]]);
            // v1 has no name_len field; derive from NUL-termination.
            let name_len = name.iter().position(|&b| b == 0).unwrap_or(ONYFS_NAME_MAX) as u8;
            Ok(OnyfsDirent {
                name,
                inode,
                dtype: 0,
                name_len,
                reserved: [0, 0],
            })
        }
        _ => {
            let off = slot * OnyfsDirent::SIZE;
            if off + OnyfsDirent::SIZE > ONYFS_BLOCK_SIZE {
                return Err(Errno::Inval);
            }
            OnyfsDirent::from_bytes(&buf_view[off..off + OnyfsDirent::SIZE]).ok_or(Errno::Io)
        }
    }
}

/// Lookup name in a directory inode. Returns inode number and fills `out` stat.
/// Supports subdirectories: if name contains '/', splits and walks.
///
/// Bug #19 fix: previously this only scanned `inode.blocks[0]`, so any
/// directory with more than ~102 entries returned NoEnt for everything
/// past the first block. We now iterate every direct block.
pub unsafe fn lookup_in(dir_ino: u32, name: &[u8], out: &mut OnyfsStat) -> KResult<u32> {
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
    // Scan every direct block of the directory inode. Skip slots that
    // are 0 (unused/deleted).
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
            // Resolve actual name length: prefer name_len field (v2), fall back to
            // NUL-termination scan (v1 / malformed v2).
            let nl = if d.name_len > 0 && (d.name_len as usize) <= ONYFS_NAME_MAX {
                d.name_len as usize
            } else {
                d.name
                    .iter()
                    .position(|&b| b == 0)
                    .unwrap_or(ONYFS_NAME_MAX)
            };
            if nl == name.len() && d.name[..nl] == *name {
                // Capture inode number BEFORE calling stat — stat() overwrites G_BUF.
                let found_ino = d.inode;
                stat(found_ino, out)?;
                return Ok(found_ino);
            }
        }
    }
    Err(Errno::NoEnt)
}

/// Lookup full path (supports subdirectories like "service/fs.bin").
pub unsafe fn lookup(path: &[u8], out: &mut OnyfsStat) -> KResult<u32> {
    let mut cur_ino = ONYFS_ROOT_INO;
    let mut remaining = path;
    loop {
        // Skip leading '/'.
        while !remaining.is_empty() && remaining[0] == b'/' {
            remaining = &remaining[1..];
        }
        if remaining.is_empty() {
            break;
        }
        // Find next '/'.
        let component = match remaining.iter().position(|&b| b == b'/') {
            Some(idx) => &remaining[..idx],
            None => remaining,
        };
        if component.is_empty() {
            break;
        }
        cur_ino = lookup_in(cur_ino, component, out)?;
        match remaining.iter().position(|&b| b == b'/') {
            Some(idx) => remaining = &remaining[idx + 1..],
            None => break,
        }
    }
    stat(cur_ino, out)?;
    Ok(cur_ino)
}

/// Resolve a directory path to inode number.
pub unsafe fn resolve_dir(path: &[u8]) -> KResult<u32> {
    let mut st = OnyfsStat::default();
    let ino = lookup(path, &mut st)?;
    if st.mode & 0o170000 != ONYFS_DT_DIR & 0o170000 {
        return Err(Errno::NotDir);
    }
    Ok(ino)
}
