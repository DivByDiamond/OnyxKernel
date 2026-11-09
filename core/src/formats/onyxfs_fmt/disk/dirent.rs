use crate::parser::le32;

use super::superblock::ONYFS_NAME_MAX;

/// OnyxFS directory entry — 40 bytes (expanded from 36 for type field).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OnyfsDirent {
    pub name: [u8; ONYFS_NAME_MAX], // 32 bytes
    pub inode: u32,                 // 4 bytes
    pub dtype: u8,                  // 1 byte: file type (REG/DIR/LNK/SNAPSHOT)
    pub name_len: u8,               // 1 byte: actual name length
    pub reserved: [u8; 2],          // 2 bytes padding
}

impl OnyfsDirent {
    pub const SIZE: usize = 40;

    pub fn from_bytes(buf: &[u8]) -> Option<Self> {
        if buf.len() < Self::SIZE {
            return None;
        }
        let mut name = [0u8; ONYFS_NAME_MAX];
        name.copy_from_slice(&buf[0..ONYFS_NAME_MAX]);
        Some(Self {
            name,
            inode: le32(&buf[32..36]),
            dtype: buf[36],
            name_len: buf[37],
            reserved: [buf[38], buf[39]],
        })
    }

    pub fn to_bytes(&self) -> [u8; 40] {
        let mut b = [0u8; 40];
        b[0..ONYFS_NAME_MAX].copy_from_slice(&self.name);
        b[32..36].copy_from_slice(&self.inode.to_le_bytes());
        b[36] = self.dtype;
        b[37] = self.name_len;
        b[38] = self.reserved[0];
        b[39] = self.reserved[1];
        b
    }

    pub fn name_str(&self) -> &[u8] {
        let n = if self.name_len > 0 && self.name_len as usize <= ONYFS_NAME_MAX {
            self.name_len as usize
        } else {
            self.name
                .iter()
                .position(|&b| b == 0)
                .unwrap_or(ONYFS_NAME_MAX)
        };
        &self.name[..n]
    }
}
