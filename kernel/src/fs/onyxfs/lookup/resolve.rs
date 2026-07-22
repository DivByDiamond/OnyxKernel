use super::super::inode::{read_inode, stat};
use super::super::{dirents_per_block, read_block, OnyfsStat, G_BUF};
use super::parse_dirent;
use onyx_core::errno::{Errno, KResult};
use onyx_core::formats::{
    OnyfsInode, ONYFS_DIRECT_BLKS, ONYFS_DT_DIR, ONYFS_NAME_MAX, ONYFS_ROOT_INO,
};

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

pub unsafe fn lookup(path: &[u8], out: &mut OnyfsStat) -> KResult<u32> {
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

pub unsafe fn resolve_dir(path: &[u8]) -> KResult<u32> {
    let mut st = OnyfsStat::default();
    let ino = lookup(path, &mut st)?;
    if st.mode & 0o170000 != ONYFS_DT_DIR & 0o170000 {
        return Err(Errno::NotDir);
    }
    Ok(ino)
}
