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
        // Bug fix (2026-09-05): same class of bug as alloc_inode() (see its
        // header comment) — this scanned the entire 4096-byte bitmap block
        // (32768 candidate data blocks) with no upper bound, so once enough
        // blocks were in use it could hand back a block number past the
        // actual data region (G_SB.total_blocks - data_blocks_start). The
        // caller (e.g. login writing /etc/passwd for the first time) then
        // silently wrote into whatever lay at that bogus block address —
        // no error surfaced, but the write never landed where a later read
        // would look, so the file came back empty. Reproduced by adding
        // enough extra files at image-build time (OnyxApps) to push past
        // the point where the first free bit the unbounded scan finds is
        // still a genuinely valid block. Cap the scan at the real data
        // block count, exactly like alloc_inode caps at inode_count.
        let data_block_count = (G_SB).total_blocks.saturating_sub((G_SB).data_blocks_start);
        if data_block_count == 0 {
            return Err(Errno::NoSpace);
        }
        let bm_blk = (G_SB).data_bitmap_start;
        let pb = &raw mut G_BUF;
        read_block(bm_blk, &mut *pb)?;
        for byte_idx in 0..ONYFS_BLOCK_SIZE {
            if (*pb)[byte_idx] == 0xFF {
                continue;
            }
            for bit in 0..8u32 {
                let bit_index = (byte_idx as u32) * 8 + bit;
                if bit_index >= data_block_count {
                    return Err(Errno::NoSpace);
                }
                if (*pb)[byte_idx] & (1 << bit) == 0 {
                    (*pb)[byte_idx] |= 1 << bit;
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
