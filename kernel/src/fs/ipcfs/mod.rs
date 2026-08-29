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

/// # Safety
///
/// Caller contract: name comes from the syscall layer's parse_user_path
/// (kernel-side slice); creates/connects an IPC channel owned by the
/// calling process (ipc::open_by_name applies its own locking).
pub unsafe fn lookup(name: &[u8]) -> KResult<u32> {
    // SAFETY: name is a kernel-side slice; open_by_name performs its own
    // channel-table lookup under the ipc module's discipline.
    unsafe {
        if name.is_empty() || name == b"" || name == b"." {
            return Ok(IPCFS_ROOT_INO);
        }
        let pid = proc::current_pid();
        let id = ipc::open_by_name(name, pid)?;
        Ok(id + 2)
    }
}

/// # Safety
///
/// No unsafe operations inside; pure ino-to-stat match with an explicit
/// chan_id bounds check (< 32). Kept unsafe for signature symmetry.
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
    let chan_id = ino - 2;
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
///
/// # Safety
///
/// Caller contract: `buf` must be writable for `len` bytes (validated and
/// translated by the syscall layer for user callers); ipc::recv bounds-checks
/// chan_id against CHAN_MAX before touching G_CHANNELS.
pub unsafe fn read(ino: u32, buf: *mut u8, _offset: u32, len: u32) -> KResult<u32> {
    // SAFETY: chan_id = ino - 2 is rejected for ino < 2, and ipc::recv
    // re-checks chan_id < CHAN_MAX before indexing G_CHANNELS; buf covers
    // `len` bytes per the caller contract.
    unsafe {
        if ino < 2 {
            return Err(Errno::Inval);
        }
        let chan_id = ino - 2;
        ipc::recv(chan_id, buf, len, None)
    }
}

/// Write to a channel (non-blocking). `ino` is the channel ID + 2.
///
/// # Safety
///
/// Caller contract: `buf` must be readable for `len` bytes (validated and
/// translated by the syscall layer for user callers); ipc::send bounds-checks
/// chan_id against CHAN_MAX before touching G_CHANNELS.
pub unsafe fn write(ino: u32, buf: *const u8, _offset: u32, len: u32) -> KResult<u32> {
    // SAFETY: chan_id = ino - 2 is rejected for ino < 2, and ipc::send
    // re-checks chan_id < CHAN_MAX before indexing G_CHANNELS; buf covers
    // `len` readable bytes per the caller contract.
    unsafe {
        if ino < 2 {
            return Err(Errno::Inval);
        }
        let chan_id = ino - 2;
        ipc::send(chan_id, buf, len, None)
    }
}

/// # Safety
///
/// Caller contract: name_out must be writable for name_len bytes (validated
/// and translated by the syscall layer for user callers); idx is a plain
/// cursor; the named-channel table walk is bounds-checked by ipc::
/// named_by_index.
pub unsafe fn readdir_entry(idx: u32, name_out: *mut u8, name_len: usize) -> Option<u32> {
    // SAFETY: name writes go through copy_name, which clamps to
    // name_len - 1 and NUL-terminates within name_len bytes.
    unsafe {
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
}

/// # Safety
///
/// Caller contract: out must be writable for max_len bytes (guaranteed by
/// the readdir_entry callers, which pass the syscall-validated buffer and
/// its length); requires max_len >= 1 so the NUL fits.
unsafe fn copy_name(name: &[u8], out: *mut u8, max_len: usize) {
    // SAFETY: n = min(name.len(), max_len - 1) < max_len, so both the copy
    // and the NUL store at index n stay within the max_len-byte buffer.
    unsafe {
        let n = name.len().min(max_len.saturating_sub(1));
        core::ptr::copy_nonoverlapping(name.as_ptr(), out, n);
        *out.add(n) = 0;
    }
}
