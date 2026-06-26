const ONYFS_BLOCK_SIZE: usize = 4096;
const ONYFS_DIRECT_BLKS: usize = 10;
const V1_INODE_SIZE: usize = 64;
const V2_INODE_SIZE: usize = 128;
const ONYFS_DT_REG: u32 = 0o100755;
const ONYFS_DT_DIR: u32 = 0o040755;

use super::tree::{DirNode, Entry};

fn write_v1(img: &mut [u8], inode_off: usize, mode: u32, size: u32, blocks: &[u32]) {
    img[inode_off..inode_off+4].copy_from_slice(&mode.to_le_bytes());
    img[inode_off+4..inode_off+8].copy_from_slice(&size.to_le_bytes());
    for (i, &blk) in blocks.iter().enumerate().take(ONYFS_DIRECT_BLKS) {
        let off = inode_off + 8 + i * 4;
        img[off..off+4].copy_from_slice(&blk.to_le_bytes());
    }
}

fn write_v2(img: &mut [u8], inode_off: usize, mode: u32, size: u64, blocks: &[u32], is_dir: bool) {
    let now_ns = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos() as u64;
    let mut buf = [0u8; V2_INODE_SIZE];
    buf[0..4].copy_from_slice(&mode.to_le_bytes());
    buf[8..16].copy_from_slice(&size.to_le_bytes());
    for (i, &blk) in blocks.iter().enumerate().take(ONYFS_DIRECT_BLKS) {
        let off = 16 + i * 4;
        buf[off..off+4].copy_from_slice(&blk.to_le_bytes());
    }
    let nlink: u32 = if is_dir { 2 } else { 1 };
    buf[64..68].copy_from_slice(&nlink.to_le_bytes());
    buf[104..112].copy_from_slice(&now_ns.to_le_bytes());
    buf[112..120].copy_from_slice(&now_ns.to_le_bytes());
    buf[120..128].copy_from_slice(&now_ns.to_le_bytes());
    img[inode_off..inode_off+V2_INODE_SIZE].copy_from_slice(&buf);
}

pub fn write_bitmaps(img: &mut [u8], inode_count: u32, data_block_count: u32) {
    for i in 0..inode_count {
        let byte_off = ONYFS_BLOCK_SIZE + (i / 8) as usize;
        img[byte_off] |= 1 << (i % 8);
    }
    for i in 0..data_block_count {
        let byte_off = 2 * ONYFS_BLOCK_SIZE + (i / 8) as usize;
        img[byte_off] |= 1 << (i % 8);
    }
}

pub fn write_table(
    img: &mut [u8], dirs: &[DirNode], files: &[Entry],
    data_blocks_start: u32, inode_table_start: u32, v1: bool,
) {
    let inode_size = if v1 { V1_INODE_SIZE } else { V2_INODE_SIZE };
    let base = inode_table_start as usize * ONYFS_BLOCK_SIZE;
    let mut data_blk = data_blocks_start;
    for d in dirs {
        let off = base + (d.ino as usize - 1) * inode_size;
        if v1 { write_v1(img, off, ONYFS_DT_DIR, 0, &[data_blk]); }
        else { write_v2(img, off, ONYFS_DT_DIR, 0, &[data_blk], true); }
        data_blk += 1;
    }
    for f in files {
        let nblks = f.data.len().div_ceil(ONYFS_BLOCK_SIZE) as u32;
        let mut blocks = [0u32; ONYFS_DIRECT_BLKS];
        for i in 0..nblks.min(ONYFS_DIRECT_BLKS as u32) {
            blocks[i as usize] = data_blk;
            data_blk += 1;
        }
        let off = base + (f.inode as usize - 1) * inode_size;
        if v1 { write_v1(img, off, ONYFS_DT_REG, f.data.len() as u32, &blocks); }
        else { write_v2(img, off, ONYFS_DT_REG, f.data.len() as u64, &blocks, false); }
    }
}
