const ONYFS_BLOCK_SIZE: usize = 4096;
const ONYFS_NAME_MAX: usize = 32;
const V1_DIRENT_SIZE: usize = 36;
const V2_DIRENT_SIZE: usize = 40;

use super::tree::{DirNode, Entry};

fn write_v1(img: &mut [u8], off: usize, name: &str, inode: u32) {
    let bytes = name.as_bytes();
    let n = bytes.len().min(ONYFS_NAME_MAX);
    img[off..off+n].copy_from_slice(&bytes[..n]);
    img[off+32..off+36].copy_from_slice(&inode.to_le_bytes());
}

fn write_v2(img: &mut [u8], off: usize, name: &str, inode: u32, is_dir: bool) {
    let bytes = name.as_bytes();
    let n = bytes.len().min(ONYFS_NAME_MAX);
    img[off..off+n].copy_from_slice(&bytes[..n]);
    img[off+32..off+36].copy_from_slice(&inode.to_le_bytes());
    img[off+36] = if is_dir { 1 } else { 2 };
    img[off+37] = n as u8;
}

pub fn write_blocks(
    img: &mut [u8], dirs: &[DirNode], files: &[Entry],
    data_blocks_start: u32, v1: bool,
) {
    let dirent_size = if v1 { V1_DIRENT_SIZE } else { V2_DIRENT_SIZE };
    let mut data_blk = data_blocks_start;
    for d in dirs {
        let dir_off = data_blk as usize * ONYFS_BLOCK_SIZE;
        data_blk += 1;
        let mut entry_off = dir_off;
        if v1 {
            write_v1(img, entry_off, ".", d.ino);
            entry_off += dirent_size;
            write_v1(img, entry_off, "..", d.parent_ino);
            entry_off += dirent_size;
        } else {
            write_v2(img, entry_off, ".", d.ino, true);
            entry_off += dirent_size;
            write_v2(img, entry_off, "..", d.parent_ino, true);
            entry_off += dirent_size;
        }
        for (name, ino, is_dir) in &d.entries {
            if entry_off + dirent_size > dir_off + ONYFS_BLOCK_SIZE { break; }
            if v1 { write_v1(img, entry_off, name, *ino); }
            else { write_v2(img, entry_off, name, *ino, *is_dir); }
            entry_off += dirent_size;
        }
    }
    for f in files {
        let nblks = f.data.len().div_ceil(ONYFS_BLOCK_SIZE);
        for i in 0..nblks {
            let blk_off = data_blk as usize * ONYFS_BLOCK_SIZE;
            data_blk += 1;
            let start = i * ONYFS_BLOCK_SIZE;
            let end = (start + ONYFS_BLOCK_SIZE).min(f.data.len());
            img[blk_off..blk_off+end-start].copy_from_slice(&f.data[start..end]);
        }
    }
}
