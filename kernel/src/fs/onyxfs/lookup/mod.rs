use super::{G_BUF, G_VERSION, ONYFS_V1, ONYFS_V1_DIRENT_SIZE};
use onyx_core::errno::{Errno, KResult};
use onyx_core::formats::{ONYFS_BLOCK_SIZE, ONYFS_NAME_MAX, OnyfsDirent};

pub(super) unsafe fn parse_dirent(slot: usize) -> KResult<OnyfsDirent> { unsafe {
    let buf_view: &[u8] = &(G_BUF);
    match G_VERSION {
        ONYFS_V1 => {
            let off = slot * ONYFS_V1_DIRENT_SIZE;
            if off + ONYFS_V1_DIRENT_SIZE > ONYFS_BLOCK_SIZE {
                return Err(Errno::Inval);
            }
            let s = &buf_view[off..off + ONYFS_V1_DIRENT_SIZE];
            let mut name = [0u8; ONYFS_NAME_MAX];
            name.copy_from_slice(&s[0..ONYFS_NAME_MAX]);
            let inode = u32::from_le_bytes([s[32], s[33], s[34], s[35]]);
            let name_len = name.iter().position(|&b| b == 0).unwrap_or(ONYFS_NAME_MAX) as u8;
            Ok(OnyfsDirent {
                name,
                inode,
                dtype: 0,
                name_len,
                reserved: [0, 0],
            })
        }
        _ => {
            let off = slot * OnyfsDirent::SIZE;
            if off + OnyfsDirent::SIZE > ONYFS_BLOCK_SIZE {
                return Err(Errno::Inval);
            }
            OnyfsDirent::from_bytes(&buf_view[off..off + OnyfsDirent::SIZE]).ok_or(Errno::Io)
        }
    }
}}

mod follow;
mod resolve;

pub use follow::*;
pub use resolve::*;
