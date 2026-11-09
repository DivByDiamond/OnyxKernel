use super::*;
use alloc::{vec, vec::Vec};
use onyx_core::formats::ONYFS_MAGIC;

#[derive(Clone)]
struct Memory {
    blocks: Vec<Block>,
    writes: usize,
    fail_at: Option<usize>,
}
impl Io for Memory {
    fn read(&mut self, n: u32, b: &mut Block) -> KResult<()> {
        *b = *self.blocks.get(n as usize).ok_or(Errno::Io)?;
        Ok(())
    }
    fn write(&mut self, n: u32, b: &Block) -> KResult<()> {
        if self.fail_at == Some(self.writes) {
            return Err(Errno::Io);
        }
        *self.blocks.get_mut(n as usize).ok_or(Errno::Io)? = *b;
        self.writes += 1;
        Ok(())
    }
}
fn volume(used: usize) -> (Memory, OnyfsSuper) {
    let mut sb = OnyfsSuper {
        magic: ONYFS_MAGIC,
        version: 2,
        block_size: 4096,
        total_blocks: 900,
        inode_count: 64,
        inode_table_start: 3,
        data_bitmap_start: 2,
        data_blocks_start: 300,
        root_inode: 1,
        snapshot_area_start: 5,
        snapshot_count: 0,
        journal_start: 261,
        journal_size: 32,
        feature_flags: ONYFS_FEAT_SNAPSHOTS,
        creation_time: 0,
        last_mount_time: 0,
        reserved: [0; 10],
    };
    let mut io = Memory {
        blocks: vec![[0; 4096]; 900],
        writes: 0,
        fail_at: None,
    };
    for i in 0..used {
        io.blocks[2][i / 8] |= 1 << (i % 8);
        // Incompressible blocks force real capacity accounting and cross-block records.
        let mut x = i as u32 + 731;
        for byte in &mut io.blocks[300 + i] {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            *byte = x as u8;
        }
    }
    io.blocks[1][0] = 7;
    io.blocks[3][0] = 23;
    io.blocks[4][4095] = 91;
    persist(&mut io, &sb).unwrap();
    io.writes = 0;
    sb.snapshot_count = 0;
    (io, sb)
}

mod create;
mod restore;
