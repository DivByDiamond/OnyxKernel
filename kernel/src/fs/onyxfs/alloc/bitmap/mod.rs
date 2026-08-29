use super::super::journal::journal_log;
use super::super::{G_BUF, G_SB, read_block, write_block};
use onyx_core::errno::{Errno, KResult};
use onyx_core::formats::ONYFS_BLOCK_SIZE;

/// # Safety
///
/// Caller must not invoke onyxfs allocation from multiple harts concurrently:
/// this mutates the module-global G_BUF scratch block and reads G_SB (set by
/// mount()). Bit indices stay in range by construction (scan of one block).
pub unsafe fn alloc_data_block() -> KResult<u32> {
    // SAFETY: single-threaded onyxfs exclusion for G_BUF/G_SB (see # Safety);
    // byte/bit indices are bounded by the 0..ONYFS_BLOCK_SIZE scan loops.
    unsafe {
        let bm_blk = (G_SB).data_bitmap_start;
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
                    return Ok((G_SB).data_blocks_start + bit_index);
                }
            }
        }
        Err(Errno::NoSpace)
    }
}

/// # Safety
///
/// Same single-threaded onyxfs exclusion contract as `alloc_data_block`.
/// `blk_num` is bounds-checked against the data region and the single-block
/// bitmap capacity before any write below.
pub unsafe fn free_data_block(blk_num: u32) -> KResult<()> {
    // SAFETY: see # Safety; blk_num has been validated (>= data_start, bit
    // index < ONYFS_BLOCK_SIZE*8) so byte_idx/bit are in range for G_BUF.
    unsafe {
        let bm_blk = (G_SB).data_bitmap_start;
        let data_start = (G_SB).data_blocks_start;
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
}

mod inode;

pub use inode::*;
