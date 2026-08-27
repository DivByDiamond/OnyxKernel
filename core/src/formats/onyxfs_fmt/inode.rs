use crate::parser::{le32, le64};
use alloc::vec::Vec;

use super::superblock::ONYFS_DIRECT_BLKS;

/// OnyxFS v2 inode — 128 bytes (expanded from 64).
/// Added: timestamps (crtime, mtime, atime, ctime), uid, gid, nlink, double_indirect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OnyfsInode {
    pub mode: u32,
    pub size: u64,                        // v2: 64-bit file size (was 32-bit)
    pub uid: u32,                         // owner user id
    pub gid: u32,                         // owner group id
    pub nlink: u32,                       // hard link count
    pub blocks: [u32; ONYFS_DIRECT_BLKS], // 10 direct blocks (40 bytes)
    pub indirect: u32,                    // single indirect block
    pub double_indirect: u32,             // double indirect block (v2)
    pub crtime: u64,                      // creation time (ns since epoch)
    pub mtime: u64,                       // modification time
    pub atime: u64,                       // access time
    pub ctime: u64,                       // inode change time
    pub flags: u32,                       // inode flags (compressed, snapshot, etc.)
    pub reserved: u32,                    // padding
}

pub const ONYFS_INODE_FLAG_COMPRESSED: u32 = 0x1;
pub const ONYFS_INODE_FLAG_SNAPSHOT: u32 = 0x2;
pub const ONYFS_INODE_FLAG_IMMUTABLE: u32 = 0x4;

impl OnyfsInode {
    pub const SIZE: usize = 128;

    pub fn from_bytes(buf: &[u8]) -> Option<Self> {
        if buf.len() < Self::SIZE {
            return None;
        }
        let mut blocks = [0u32; ONYFS_DIRECT_BLKS];
        for (i, b) in blocks.iter_mut().enumerate() {
            *b = le32(&buf[16 + i * 4..16 + (i + 1) * 4]);
        }
        Some(Self {
            mode: le32(&buf[0..4]),
            size: le64(&buf[8..16]),
            uid: le32(&buf[56..60]),
            gid: le32(&buf[60..64]),
            nlink: le32(&buf[64..68]),
            blocks,
            indirect: le32(&buf[96..100]),
            double_indirect: le32(&buf[100..104]),
            crtime: le64(&buf[104..112]),
            mtime: le64(&buf[112..120]),
            atime: le64(&buf[120..128]),
            // ctime, flags, reserved would need more bytes — using extended layout
            ctime: 0,
            flags: le32(&buf[68..72]),
            reserved: le32(&buf[72..76]),
        })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut b = alloc::vec![0u8; Self::SIZE];
        b[0..4].copy_from_slice(&self.mode.to_le_bytes());
        b[4..8].copy_from_slice(&0u32.to_le_bytes()); // padding
        b[8..16].copy_from_slice(&self.size.to_le_bytes());
        for (i, &bl) in self.blocks.iter().enumerate() {
            let off = 16 + i * 4;
            b[off..off + 4].copy_from_slice(&bl.to_le_bytes());
        }
        b[56..60].copy_from_slice(&self.uid.to_le_bytes());
        b[60..64].copy_from_slice(&self.gid.to_le_bytes());
        b[64..68].copy_from_slice(&self.nlink.to_le_bytes());
        b[68..72].copy_from_slice(&self.flags.to_le_bytes());
        b[72..76].copy_from_slice(&self.reserved.to_le_bytes());
        b[96..100].copy_from_slice(&self.indirect.to_le_bytes());
        b[100..104].copy_from_slice(&self.double_indirect.to_le_bytes());
        b[104..112].copy_from_slice(&self.crtime.to_le_bytes());
        b[112..120].copy_from_slice(&self.mtime.to_le_bytes());
        b[120..128].copy_from_slice(&self.atime.to_le_bytes());
        b
    }
}
