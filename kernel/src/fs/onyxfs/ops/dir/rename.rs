use super::super::super::OnyfsStat;
use super::super::super::alloc::{add_dirent, free_inode, remove_dirent};
use super::super::super::journal::journal_commit;
use super::super::super::lookup::{lookup, lookup_nofollow};
use onyx_core::errno::{Errno, KResult};
use onyx_core::formats::ONYFS_ROOT_INO;

/// # Safety
///
/// Caller must not invoke onyxfs operations concurrently from multiple harts
/// (shared G_BUF scratch global, journal head, dirent slots). `old_path` and
/// `new_path` must be kernel-owned, absolute (leading '/'), NUL-free byte
/// strings - the syscall layer parses user paths before reaching fs/.
pub unsafe fn rename(old_path: &[u8], new_path: &[u8]) -> KResult<()> {
    // SAFETY: single-threaded onyxfs exclusion (see # Safety); all raw access
    // is delegated to the bounds-checked lookup/add_dirent/remove_dirent/
    // journal helpers, and path slicing stays within path.len().
    unsafe {
        crate::fs::onyxfs::journal::transaction(|| {
            if old_path.is_empty()
                || old_path[0] != b'/'
                || new_path.is_empty()
                || new_path[0] != b'/'
            {
                return Err(Errno::Inval);
            }
            let mut st = OnyfsStat::default();
            let old_ino = lookup_nofollow(old_path, &mut st)?;
            let dtype = ((st.mode >> 12) & 0xF) as u8;

            let old_trimmed = &old_path[1..];
            let old_last_slash = old_trimmed.iter().rposition(|&b| b == b'/');
            let (old_parent_path, old_filename) = match old_last_slash {
                Some(pos) => (&old_trimmed[..pos], &old_trimmed[pos + 1..]),
                None => (&b""[..], old_trimmed),
            };

            let new_trimmed = &new_path[1..];
            let new_last_slash = new_trimmed.iter().rposition(|&b| b == b'/');
            let (new_parent_path, new_filename) = match new_last_slash {
                Some(pos) => (&new_trimmed[..pos], &new_trimmed[pos + 1..]),
                None => (&b""[..], new_trimmed),
            };

            if old_filename.is_empty() || new_filename.is_empty() {
                return Err(Errno::Inval);
            }

            let old_parent_ino = if old_parent_path.is_empty() {
                ONYFS_ROOT_INO
            } else {
                lookup(old_parent_path, &mut st)?;
                st.ino
            };

            let new_parent_ino = if new_parent_path.is_empty() {
                ONYFS_ROOT_INO
            } else {
                lookup(new_parent_path, &mut st)?;
                st.ino
            };

            // add_dirent overwrites an existing entry in place, so the inode that the
            // name pointed to must be released here or it stays allocated forever
            // (the guest's passwd/shadow temp-file renames leaked one per rewrite).
            let mut victim_st = OnyfsStat::default();
            let victim = lookup_nofollow(new_path, &mut victim_st).ok();
            if let Some(victim_ino) = victim {
                if victim_ino == old_ino {
                    return Ok(());
                }
                let old_is_dir = u32::from(dtype) == 0o040000 >> 12;
                let victim_is_dir = victim_st.mode & 0o170000 == 0o040000;
                match (old_is_dir, victim_is_dir) {
                    (false, true) => return Err(Errno::IsDir),
                    (true, false) => return Err(Errno::NotDir),
                    (true, true) => return Err(Errno::Exist),
                    (false, false) => {}
                }
            }

            add_dirent(new_parent_ino, new_filename, old_ino, dtype)?;
            remove_dirent(old_parent_ino, old_filename)?;
            if let Some(victim_ino) = victim {
                free_inode(victim_ino)?;
            }
            journal_commit()?;
            Ok(())
        })
    }
}
