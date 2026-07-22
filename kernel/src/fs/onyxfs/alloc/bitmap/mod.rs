use super::super::journal::journal_log;
use super::super::{read_block, write_block, G_BUF, G_SB};
use onyx_core::errno::{Errno, KResult};
use onyx_core::formats::ONYFS_BLOCK_SIZE;

pub unsafe fn alloc_data_block() -> KResult<u32> {
    let bm_blk = (*(&raw const G_SB)).data_bitmap_start;
    let pb = &raw mut G_BUF;
    read_block(bm_blk, &mut *pb)?;
    for byte_idx in 0..ONYFS_BLOCK_SIZE {
        if (*pb)[byte_idx] == 0xFF {
            continue;
        }
        for bit in 0..8u32 {
            if (*pb)[byte_idx] & (1 << bit) == 0 {
                (*pb)[byte_idx] |= 1 << bit;
                let bit_index = (byte_idx as u32) * 8 + bit;
                journal_log(bm_blk, &*pb)?;
                write_block(bm_blk, &*pb)?;
                return Ok((*(&raw const G_SB)).data_blocks_start + bit_index);
            }
        }
    }
    Err(Errno::NoSpace)
}

pub unsafe fn free_data_block(blk_num: u32) -> KResult<()> {
    let bm_blk = (*(&raw const G_SB)).data_bitmap_start;
    let data_start = (*(&raw const G_SB)).data_blocks_start;
    if blk_num < data_start {
        return Err(Errno::Inval);
    }
    let bit_index = (blk_num - data_start) as usize;
    let max_bits = ONYFS_BLOCK_SIZE * 8;
    if bit_index >= max_bits {
        return Err(Errno::Inval);
    }
    let byte_idx = bit_index / 8;
    let bit = (bit_index % 8) as u8;
    let pb = &raw mut G_BUF;
    read_block(bm_blk, &mut *pb)?;
    (*pb)[byte_idx] &= !(1 << bit);
    journal_log(bm_blk, &*pb)?;
    write_block(bm_blk, &*pb)?;
    Ok(())
}

mod inode;

pub use inode::*;
