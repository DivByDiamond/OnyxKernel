use super::super::OnyfsStat;
use super::super::inode::stat;
use super::super::symlink::readlink;
use super::lookup_in;
use onyx_core::errno::{Errno, KResult};
use onyx_core::formats::{ONYFS_DT_LNK, ONYFS_ROOT_INO};

/// # Safety
///
/// Caller must not invoke onyxfs operations concurrently from multiple harts
/// (shared G_BUF scratch global on every block read). `path` must be a
/// kernel-owned byte string (the syscall layer parses user paths before
/// reaching fs/); recursion is depth-capped at 8 below.
pub unsafe fn lookup_follow(path: &[u8], out: &mut OnyfsStat, depth: u32) -> KResult<u32> {
    // SAFETY: single-threaded onyxfs exclusion (see # Safety); all raw access
    // goes through the bounds-checked lookup_in/stat/readlink helpers; the
    // stack buffers are written only within their computed lengths.
    unsafe {
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
                // Link targets are capped at SYMLINK_TARGET_MAX at creation time
                // (sys_symlink parses the target into a 256-byte buffer), so a
                // 256-byte read covers every link that userland can create.
                // Small buffers keep the per-recursion frame ~1.3 KB: at the
                // depth cap of 8 this stays far below the 64 KB kernel stack.
                const SYMLINK_TARGET_MAX: usize = 256;
                const RECOMPOSED_MAX: usize = 1024;
                let mut link_target = [0u8; SYMLINK_TARGET_MAX];
                let link_len =
                    readlink(cur_ino, link_target.as_mut_ptr(), SYMLINK_TARGET_MAX as u32)?
                        as usize;
                let target = &link_target[..link_len];
                let mut new_path = [0u8; RECOMPOSED_MAX];
                let mut pos = 0;
                if target.first() == Some(&b'/') {
                    let n = target.len().min(RECOMPOSED_MAX - pos);
                    new_path[pos..pos + n].copy_from_slice(&target[..n]);
                    pos += n;
                    let rest = &path[offset..];
                    let n = rest.len().min(RECOMPOSED_MAX - pos);
                    new_path[pos..pos + n].copy_from_slice(&rest[..n]);
                    pos += n;
                } else {
                    let parent_end = comp_start.saturating_sub(1);
                    let parent = &path[..parent_end];
                    if !parent.is_empty() {
                        let n = parent.len().min(RECOMPOSED_MAX - pos);
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
                    let n = target.len().min(RECOMPOSED_MAX - pos);
                    new_path[pos..pos + n].copy_from_slice(&target[..n]);
                    pos += n;
                    let rest = &path[offset..];
                    let n = rest.len().min(RECOMPOSED_MAX - pos);
                    new_path[pos..pos + n].copy_from_slice(&rest[..n]);
                    pos += n;
                }
                return lookup_follow(&new_path[..pos], out, depth + 1);
            }
        }
        stat(cur_ino, out)?;
        Ok(cur_ino)
    }
}
