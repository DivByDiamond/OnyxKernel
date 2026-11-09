//! FAT32 directory operations: path resolution and directory iteration.
pub(crate) mod lfn;
mod path;
mod read;
mod scan;

use super::lfn::LfnChain;
use super::{
    ATTR_DIRECTORY, ATTR_LFN, DIR_ENTRY_SIZE, ENTRIES_PER_SECTOR, G_SPC, fat_entry, is_eoc,
    is_valid_cluster, read_cluster_sector,
};

pub(crate) use scan::scan_dir_entries;

pub use path::*;
pub use read::*;

/// Read one directory entry by index (stateful cursor maintained by VFS).
/// Writes the long name when a valid LFN chain precedes the entry, otherwise
/// the 8.3-derived lowercase name (NUL-terminated), into `name_out`, and
/// returns the entry's cluster number plus its d_type (DT_DIR/DT_REG from
/// the attribute byte), or None past end-of-directory.
///
/// # Safety
///
/// `name_out` must be writable for `name_len` bytes and stay valid for the
/// call (the syscall layer translates user buffers before reaching fs/);
/// every write below is capped at name_len - 1 plus a NUL. Caller must not
/// invoke FAT32 I/O concurrently from multiple harts.
pub unsafe fn readdir_entry(
    dir_cluster: u32,
    entry_idx: u32,
    name_out: *mut u8,
    name_len: usize,
) -> Option<(u32, u8)> {
    // SAFETY: see # Safety for name_out validity - out_pos is checked against
    // name_len before each byte write; cluster numbers are validated via
    // is_valid_cluster before each chain hop.
    unsafe {
        if !is_valid_cluster(dir_cluster) {
            return None;
        }
        let mut buf = [0u8; 512];
        let mut chain = LfnChain::new();
        let mut long_name = [0u8; 255];
        let mut cluster = dir_cluster;
        let mut cursor = 0u32;
        let mut hop = 0u32;
        const MAX_HOPS: u32 = 65536;
        loop {
            if hop >= MAX_HOPS {
                return None;
            }
            hop += 1;
            for si in 0..G_SPC {
                if read_cluster_sector(cluster, si, &mut buf).is_err() {
                    return None;
                }
                for ei in 0..ENTRIES_PER_SECTOR {
                    let off = ei * DIR_ENTRY_SIZE;
                    let attr = buf[off + 11];
                    let dtype = if attr & ATTR_DIRECTORY != 0 {
                        crate::fs::DT_DIR
                    } else {
                        crate::fs::DT_REG
                    };
                    if attr == ATTR_LFN {
                        chain.push(&buf[off..off + DIR_ENTRY_SIZE]);
                        continue;
                    }
                    if buf[off] == 0 {
                        return None;
                    }
                    if buf[off] == 0xE5 {
                        chain.reset();
                        continue;
                    }
                    if cursor < entry_idx {
                        cursor += 1;
                        chain.reset();
                        continue;
                    }
                    let mut short_name = [0u8; 11];
                    short_name.copy_from_slice(&buf[off..off + 11]);
                    let long_len = if chain.belongs_to(&short_name) {
                        chain.decode(&mut long_name)
                    } else {
                        None
                    };
                    chain.reset();
                    if let Some(long_len) = long_len {
                        let mut out_pos = 0usize;
                        for &byte in long_name.iter().take(long_len) {
                            if out_pos >= name_len.saturating_sub(1) {
                                break;
                            }
                            name_out.add(out_pos).write(byte);
                            out_pos += 1;
                        }
                        if out_pos < name_len {
                            name_out.add(out_pos).write(0);
                        }
                        let cluster_lo = u16::from_le_bytes([buf[off + 26], buf[off + 27]]);
                        let cluster_hi = u16::from_le_bytes([buf[off + 20], buf[off + 21]]);
                        let cluster_num = ((cluster_hi as u32) << 16) | cluster_lo as u32;
                        return Some((
                            if cluster_num == 0 {
                                dir_cluster
                            } else {
                                cluster_num
                            },
                            dtype,
                        ));
                    }
                    let mut out_pos = 0usize;
                    let mut base_end = 8usize;
                    for i in (0..8).rev() {
                        if buf[off + i] != b' ' {
                            base_end = i + 1;
                            break;
                        }
                    }
                    for i in 0..base_end {
                        if out_pos >= name_len.saturating_sub(1) {
                            break;
                        }
                        let c = buf[off + i];
                        name_out.add(out_pos).write(if c.is_ascii_uppercase() {
                            c + 32
                        } else {
                            c
                        });
                        out_pos += 1;
                    }
                    let mut ext_end = 11usize;
                    for i in (8..11).rev() {
                        if buf[off + i] != b' ' {
                            ext_end = i + 1;
                            break;
                        }
                    }
                    if ext_end > 8 {
                        if out_pos < name_len.saturating_sub(1) {
                            name_out.add(out_pos).write(b'.');
                            out_pos += 1;
                        }
                        for i in 8..ext_end {
                            if out_pos >= name_len.saturating_sub(1) {
                                break;
                            }
                            let c = buf[off + i];
                            name_out.add(out_pos).write(if c.is_ascii_uppercase() {
                                c + 32
                            } else {
                                c
                            });
                            out_pos += 1;
                        }
                    }
                    if out_pos < name_len {
                        name_out.add(out_pos).write(0);
                    }
                    let cluster_lo = u16::from_le_bytes([buf[off + 26], buf[off + 27]]);
                    let cluster_hi = u16::from_le_bytes([buf[off + 20], buf[off + 21]]);
                    let cluster_num = ((cluster_hi as u32) << 16) | cluster_lo as u32;
                    return Some((
                        if cluster_num == 0 {
                            dir_cluster
                        } else {
                            cluster_num
                        },
                        dtype,
                    ));
                }
            }
            let next = fat_entry(cluster, &mut buf);
            if is_eoc(next) {
                return None;
            }
            if !is_valid_cluster(next) {
                return None;
            }
            cluster = next;
        }
    }
}
