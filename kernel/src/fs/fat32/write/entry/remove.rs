//! Directory-entry removal: tombstone the short entry and its LFN fragments,
//! then free the cluster chain.
use onyx_core::errno::{Errno, KResult};

use super::super::super::dir::lfn::{LFN_MAX_ENTRIES, LfnChain, name_eq};
use super::super::super::{
    ATTR_LFN, DIR_ENTRY_SIZE, ENTRIES_PER_SECTOR, G_SPC, fat_entry, fat32_name_8_3, is_eoc,
    is_valid_cluster, read_cluster_sector,
};
use super::super::alloc::chain::free_chain;
use super::super::alloc::fat::write_cluster_sector;

/// Entry name byte that marks a deleted entry.
const DELETED: u8 = 0xE5;
/// Entry name byte that marks end-of-directory.
const DIRENT_END: u8 = 0x00;
/// Where one LFN fragment currently lives.
type Slot = (u32, u32, usize);

/// Mark the entry named `name` (8.3 alias or long name) as deleted and free
/// its clusters. Returns `Ok(())` if found, `Err(NoEnt)` otherwise.
///
/// The short entry is tombstoned first, then its LFN fragments, then the
/// chain is freed: a crash in between hides the file before its clusters can
/// be reused, so it can only leak, never corrupt another file.
///
/// # Safety
///
/// Caller must not invoke FAT32 I/O concurrently from multiple harts
/// (two racing unlinks on the same entry could free the chain twice);
/// `dir_cluster` must be a valid data cluster and is re-validated on every
/// chain hop below.
pub unsafe fn unlink(dir_cluster: u32, name: &[u8]) -> KResult<()> {
    // SAFETY: single-threaded FAT32 exclusion (see # Safety); entry offsets
    // are ei*32 with ei < 16, staying inside the 512-byte sector buffer.
    unsafe {
        // Clearing slots shifts every later position: drop the readdir
        // resume point before mutating the directory.
        super::super::super::dir::invalidate_walk();
        if name.is_empty() {
            return Err(Errno::Inval);
        }
        let needle = fat32_name_8_3(name);
        let mut buf = [0u8; 512];
        let mut chain = LfnChain::new();
        let mut long_name = [0u8; 255];
        let mut run = [Slot::default(); LFN_MAX_ENTRIES];
        let mut run_len = 0usize;
        let mut cluster = dir_cluster;
        let mut hop = 0u32;
        const MAX_HOPS: u32 = 65536;
        loop {
            hop += 1;
            if hop >= MAX_HOPS {
                return Err(Errno::Io);
            }
            for si in 0..G_SPC {
                read_cluster_sector(cluster, si, &mut buf)?;
                for ei in 0..ENTRIES_PER_SECTOR {
                    let off = ei * DIR_ENTRY_SIZE;
                    let attr = buf[off + 11];
                    if attr == ATTR_LFN {
                        chain.push(&buf[off..off + DIR_ENTRY_SIZE]);
                        if run_len < LFN_MAX_ENTRIES {
                            run[run_len] = (cluster, si, ei);
                            run_len += 1;
                        }
                        continue;
                    }
                    if buf[off] == DIRENT_END {
                        return Err(Errno::NoEnt);
                    }
                    if buf[off] == DELETED {
                        chain.reset();
                        run_len = 0;
                        continue;
                    }
                    let mut entry = [0u8; 11];
                    entry.copy_from_slice(&buf[off..off + 11]);
                    let short_match = entry == needle;
                    let long_match = chain.belongs_to(&entry)
                        && chain
                            .decode(&mut long_name)
                            .is_some_and(|len| name_eq(&long_name[..len], name));
                    if short_match || long_match {
                        let cluster_lo = u16::from_le_bytes([buf[off + 26], buf[off + 27]]);
                        let cluster_hi = u16::from_le_bytes([buf[off + 20], buf[off + 21]]);
                        let file_cluster = ((cluster_hi as u32) << 16) | cluster_lo as u32;
                        buf[off] = DELETED;
                        write_cluster_sector(cluster, si, &buf)?;
                        for &(frag_cluster, frag_sec, frag_idx) in run[..run_len].iter() {
                            let mut frag = [0u8; 512];
                            read_cluster_sector(frag_cluster, frag_sec, &mut frag)?;
                            frag[frag_idx * DIR_ENTRY_SIZE] = DELETED;
                            write_cluster_sector(frag_cluster, frag_sec, &frag)?;
                        }
                        if file_cluster != 0 {
                            free_chain(file_cluster)?;
                        }
                        return Ok(());
                    }
                    chain.reset();
                    run_len = 0;
                }
            }
            let next = fat_entry(cluster, &mut buf);
            if is_eoc(next) || !is_valid_cluster(next) {
                return Err(Errno::NoEnt);
            }
            cluster = next;
        }
    }
}
