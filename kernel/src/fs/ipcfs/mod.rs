//! ipcfs — virtual filesystem exposing named IPC channels.
//!
//! Mounted at `/ipc` by the VFS mount table. Each named channel appears as
//! a file. Opening `/ipc/<name>` connects the calling process to the channel.
//! Reading from the file receives data; writing sends data.
//!
//! Inode layout:
//!   1 → /ipc (directory)
//!   2+ → (channel_id + 2) mapped to each named channel

use crate::ipc;
use crate::proc;
use onyx_core::errno::{Errno, KResult};
use onyx_core::formats::ONYFS_ROOT_INO;

pub const IPCFS_ROOT_INO: u32 = ONYFS_ROOT_INO;

const IPCFS_MAX_SIZE: u32 = 4096;

pub struct IpcfsStat {
    pub ino: u32,
    pub size: u32,
    pub mode: u32,
}

pub unsafe fn lookup(name: &[u8]) -> KResult<u32> {
    if name.is_empty() || name == b"" || name == b"." {
        return Ok(IPCFS_ROOT_INO);
    }
    let pid = proc::current_pid();
    let id = ipc::open_by_name(name, pid)?;
    Ok(id + 2)
}

pub unsafe fn stat(ino: u32) -> KResult<IpcfsStat> {
    if ino == IPCFS_ROOT_INO {
        return Ok(IpcfsStat {
            ino,
            size: 0,
            mode: 0o040755,
        });
    }
    if ino < 2 {
        return Err(Errno::NoEnt);
    }
    let chan_id = (ino - 2) as u32;
    if chan_id >= 32 {
        return Err(Errno::NoEnt);
    }
    Ok(IpcfsStat {
        ino,
        size: IPCFS_MAX_SIZE,
        mode: 0o100666,
    })
}

/// Read from a channel (non-blocking). `ino` is the channel ID + 2.
pub unsafe fn read(ino: u32, buf: *mut u8, _offset: u32, len: u32) -> KResult<u32> {
    if ino < 2 {
        return Err(Errno::Inval);
    }
    let chan_id = (ino - 2) as u32;
    ipc::recv(chan_id, buf, len, None)
}

/// Write to a channel (non-blocking). `ino` is the channel ID + 2.
pub unsafe fn write(ino: u32, buf: *const u8, _offset: u32, len: u32) -> KResult<u32> {
    if ino < 2 {
        return Err(Errno::Inval);
    }
    let chan_id = (ino - 2) as u32;
    ipc::send(chan_id, buf, len, None)
}

pub unsafe fn readdir_entry(idx: u32, name_out: *mut u8, name_len: usize) -> Option<u32> {
    match idx {
        0 => {
            let name = b".";
            copy_name(name, name_out, name_len);
            Some(IPCFS_ROOT_INO)
        }
        1 => {
            let name = b"..";
            copy_name(name, name_out, name_len);
            Some(IPCFS_ROOT_INO)
        }
        _ => {
            let entry_idx = idx - 2;
            if let Some((name, chan_id)) = ipc::named_by_index(entry_idx) {
                copy_name(name, name_out, name_len);
                Some(chan_id + 2)
            } else {
                None
            }
        }
    }
}

unsafe fn copy_name(name: &[u8], out: *mut u8, max_len: usize) {
    let n = name.len().min(max_len.saturating_sub(1));
    core::ptr::copy_nonoverlapping(name.as_ptr(), out, n);
    *out.add(n) = 0;
}
