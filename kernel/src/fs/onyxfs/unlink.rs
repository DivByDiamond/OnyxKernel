use super::OnyfsStat;
use super::alloc::{free_inode, remove_dirent};
use super::journal::journal_commit;
use super::lookup::lookup;
use onyx_core::errno::{Errno, KResult};
use onyx_core::formats::ONYFS_ROOT_INO;

pub unsafe fn unlink(path: &[u8]) -> KResult<()> {
    if path.is_empty() || path[0] != b'/' {
        return Err(Errno::Inval);
    }
    if path.len() == 1 {
        return Err(Errno::Perm);
    }
    let trimmed = &path[1..];
    let last_slash = trimmed.iter().rposition(|&b| b == b'/');
    let (parent_path, filename) = match last_slash {
        Some(pos) => (&trimmed[..pos], &trimmed[pos + 1..]),
        None => (&b""[..], trimmed),
    };
    if filename.is_empty() {
        return Err(Errno::Inval);
    }
    let parent_ino = if parent_path.is_empty() {
        ONYFS_ROOT_INO
    } else {
        let mut st = OnyfsStat::default();
        lookup(parent_path, &mut st)?;
        st.ino
    };
    // Resolve the target inode BEFORE removing the dirent — we need its
    // inode number to free it. The previous code skipped this entirely,
    // so unlink only zeroed the dirent's inode field and leaked the
    // inode + every data block it referenced (Bug #20).
    let mut target_st = OnyfsStat::default();
    let target_ino = match lookup(path, &mut target_st) {
        Ok(ino) => ino,
        // Already gone — nothing to do.
        Err(_) => return Err(Errno::NoEnt),
    };
    // Refuse to unlink the root or a directory via this path. Directory
    // removal should go through rmdir (TODO). The mode check uses the
    // S_IFMT bits (0o170000); S_IFDIR = 0o040000.
    if target_st.mode & 0o170000 == 0o040000 {
        return Err(Errno::IsDir);
    }
    remove_dirent(parent_ino, filename)?;
    // Now actually reclaim the inode and its data blocks.
    free_inode(target_ino)?;
    journal_commit()?;
    Ok(())
}
