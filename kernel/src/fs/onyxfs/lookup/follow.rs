use super::super::inode::stat;
use super::super::symlink::readlink;
use super::super::OnyfsStat;
use super::lookup_in;
use onyx_core::errno::{Errno, KResult};
use onyx_core::formats::{ONYFS_BLOCK_SIZE, ONYFS_DT_LNK, ONYFS_ROOT_INO};

pub unsafe fn lookup_follow(path: &[u8], out: &mut OnyfsStat, depth: u32) -> KResult<u32> {
    if depth > 8 {
        return Err(Errno::Loop);
    }
    let mut cur_ino = ONYFS_ROOT_INO;
    let mut offset: usize = 0;
    let path_len = path.len();
    loop {
        while offset < path_len && path[offset] == b'/' {
            offset += 1;
        }
        if offset >= path_len {
            break;
        }
        let comp_start = offset;
        while offset < path_len && path[offset] != b'/' {
            offset += 1;
        }
        let component = &path[comp_start..offset];
        if component.is_empty() {
            break;
        }
        let mut tmp = OnyfsStat::default();
        cur_ino = lookup_in(cur_ino, component, &mut tmp)?;
        if tmp.mode & 0o170000 == ONYFS_DT_LNK & 0o170000 {
            let mut link_target = [0u8; ONYFS_BLOCK_SIZE];
            let link_len =
                readlink(cur_ino, link_target.as_mut_ptr(), ONYFS_BLOCK_SIZE as u32)? as usize;
            let target = &link_target[..link_len];
            let max_new = ONYFS_BLOCK_SIZE + 256;
            let mut new_path = [0u8; 8192];
            let mut pos = 0;
            if target.first() == Some(&b'/') {
                let n = target.len().min(max_new - pos);
                new_path[pos..pos + n].copy_from_slice(&target[..n]);
                pos += n;
                let rest = &path[offset..];
                let n = rest.len().min(max_new - pos);
                new_path[pos..pos + n].copy_from_slice(&rest[..n]);
                pos += n;
            } else {
                let parent_end = comp_start.saturating_sub(1);
                let parent = &path[..parent_end];
                if !parent.is_empty() {
                    let n = parent.len().min(max_new - pos);
                    new_path[pos..pos + n].copy_from_slice(&parent[..n]);
                    pos += n;
                    if new_path[pos - 1] != b'/' {
                        new_path[pos] = b'/';
                        pos += 1;
                    }
                } else {
                    new_path[pos] = b'/';
                    pos += 1;
                }
                let n = target.len().min(max_new - pos);
                new_path[pos..pos + n].copy_from_slice(&target[..n]);
                pos += n;
                let rest = &path[offset..];
                let n = rest.len().min(max_new - pos);
                new_path[pos..pos + n].copy_from_slice(&rest[..n]);
                pos += n;
            }
            return lookup_follow(&new_path[..pos], out, depth + 1);
        }
    }
    stat(cur_ino, out)?;
    Ok(cur_ino)
}
