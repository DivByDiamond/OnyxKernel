//! PTY device nodes in devfs: /dev/ptmx (master clone) and /dev/pts/N
//! (slave terminals). Inode layout mirrors the block-device scheme: a
//! dedicated clone inode for ptmx plus a base-inode range per index.
//!
//! Opening /dev/ptmx allocates a fresh pair and rewrites the fd's inode
//! to the master slot for that pair (vfs/fd/file.rs hook). /dev/pts/N
//! only resolves while pair N is live. Closing the master frees the pair
//! (pty::free), after which slave I/O fails with EPIPE.

use crate::fs::pty;
use onyx_core::errno::{Errno, KResult};

/// The clone inode: every open("/dev/ptmx") allocates a new pair.
pub const DEVFS_PTMX_INO: u32 = 48;
/// Master fd inodes: DEVFS_PTYM_BASE_INO + idx (idx < PTY_MAX).
pub const DEVFS_PTYM_BASE_INO: u32 = 32;
/// Slave fd inodes: DEVFS_PTYS_BASE_INO + idx.
pub const DEVFS_PTYS_BASE_INO: u32 = 40;

/// True when `ino` is any PTY node (used by the syscall read/write hooks).
pub fn is_pty_ino(ino: u32) -> bool {
    ino == DEVFS_PTMX_INO || ptym_ino_idx(ino).is_some() || ptys_ino_idx(ino).is_some()
}

/// Master inode -> pair index.
pub fn ptym_ino_idx(ino: u32) -> Option<u32> {
    if ino >= DEVFS_PTYM_BASE_INO && ino < DEVFS_PTYM_BASE_INO + pty::PTY_MAX as u32 {
        Some(ino - DEVFS_PTYM_BASE_INO)
    } else {
        None
    }
}

/// Slave inode -> pair index (only while the pair is live).
pub fn ptys_ino_idx(ino: u32) -> Option<u32> {
    if ino >= DEVFS_PTYS_BASE_INO && ino < DEVFS_PTYS_BASE_INO + pty::PTY_MAX as u32 {
        let idx = ino - DEVFS_PTYS_BASE_INO;
        // SAFETY: idx < PTY_MAX by the range check above; syscall or fd
        // setup context on this hart.
        if unsafe { pty::is_used(idx) } {
            return Some(idx);
        }
    }
    None
}

/// Master pair index -> inode (used when open(/dev/ptmx) claims a pair).
pub fn ptym_ino(idx: u32) -> u32 {
    DEVFS_PTYM_BASE_INO + idx
}

/// devfs::lookup hook: recognize "ptmx" and "pts/N" names.
pub fn lookup_name(name: &[u8]) -> Option<u32> {
    if name == b"ptmx" {
        return Some(DEVFS_PTMX_INO);
    }
    let digits = name.strip_prefix(b"pts/")?;
    let idx = parse_pts_index(digits)?;
    if ptys_ino_idx(DEVFS_PTYS_BASE_INO + idx).is_some() {
        Some(DEVFS_PTYS_BASE_INO + idx)
    } else {
        None
    }
}

/// Parse a decimal pts index ("0".."3"); None on empty/nondigit/oversized.
fn parse_pts_index(digits: &[u8]) -> Option<u32> {
    if digits.is_empty() || !digits.iter().all(|d| d.is_ascii_digit()) {
        return None;
    }
    let mut n: u32 = 0;
    for &d in digits {
        n = n.checked_mul(10)?.checked_add((d - b'0') as u32)?;
        if n >= pty::PTY_MAX as u32 {
            return None;
        }
    }
    Some(n)
}

/// devfs::stat hook for PTY nodes. Streams report a huge size so the
/// positional fd layer never truncates reads before the pty backend sees
/// them (same trick as the block devices, which use u32::MAX).
pub fn stat(ino: u32) -> Option<(u32, u32)> {
    if is_pty_ino(ino) {
        Some((ino, u32::MAX))
    } else {
        None
    }
}

/// devfs::read hook.
///
/// # Safety
///
/// `buf` must be writable for `len` bytes (validated by the syscall layer);
/// ino comes from a devfs fd slot; syscall context only.
pub unsafe fn read(ino: u32, buf: *mut u8, _offset: u32, len: u32) -> KResult<u32> {
    // SAFETY: buffer validity is the caller contract; side_read re-checks
    // the pair index and liveness under the pair lock.
    unsafe {
        if let Some(idx) = ptym_ino_idx(ino) {
            return pty::side_read(idx, true, buf, len);
        }
        if let Some(idx) = ptys_ino_idx(ino) {
            return pty::side_read(idx, false, buf, len);
        }
        Err(Errno::NoEnt)
    }
}

/// devfs::write hook.
///
/// # Safety
///
/// `buf` must be readable for `len` bytes (validated by the syscall layer);
/// ino comes from a devfs fd slot; syscall context only.
pub unsafe fn write(ino: u32, buf: *const u8, _offset: u32, len: u32) -> KResult<u32> {
    // SAFETY: buffer validity is the caller contract; side_write re-checks
    // the pair index and liveness under the pair lock.
    unsafe {
        if let Some(idx) = ptym_ino_idx(ino) {
            return pty::side_write(idx, true, buf, len);
        }
        if let Some(idx) = ptys_ino_idx(ino) {
            return pty::side_write(idx, false, buf, len);
        }
        Err(Errno::NoEnt)
    }
}

/// poll() readiness for a PTY fd: Some((readable, writable)).
///
/// # Safety
///
/// syscall context only; pair liveness checked inside.
pub unsafe fn pty_poll(ino: u32) -> Option<(bool, bool)> {
    // SAFETY: pair index derived from ino ranges; side_poll re-checks the
    // index and liveness under the pair lock.
    unsafe {
        if let Some(idx) = ptym_ino_idx(ino) {
            return Some(pty::side_poll(idx, true));
        }
        if let Some(idx) = ptys_ino_idx(ino) {
            return Some(pty::side_poll(idx, false));
        }
        None
    }
}

/// devfs::readdir_entry hook: ptmx sits right after the block devices,
/// then one entry per live pts/N. Returns the inode for name_out.
pub fn readdir_entry(idx: u32, name_out: *mut u8, name_len: usize) -> Option<u32> {
    let ptmx_slot = 3 + crate::drivers::virtio::count() as u32;
    if idx == ptmx_slot {
        copy_name(b"ptmx", name_out, name_len);
        return Some(DEVFS_PTMX_INO);
    }
    let pts_base = ptmx_slot + 1;
    if idx >= pts_base {
        let n = idx - pts_base;
        if (n as usize) < pty::PTY_MAX {
            // SAFETY: n < PTY_MAX by the check above; is_used re-checks the
            // bounds and liveness internally.
            if unsafe { pty::is_used(n) } {
                copy_name(&pts_name(n), name_out, name_len);
                return Some(DEVFS_PTYS_BASE_INO + n);
            }
        }
    }
    None
}

fn pts_name(n: u32) -> [u8; 5] {
    // "pts/N" with a single-digit N (PTY_MAX <= 4).
    let mut out = *b"pts/0";
    out[4] = b'0' + n as u8;
    out
}

fn copy_name(name: &[u8], out: *mut u8, max_len: usize) {
    let n = name.len().min(max_len);
    // SAFETY: n <= max_len by construction, so the copy stays within the
    // caller-provided name_out buffer (same contract as devfs::copy_name).
    unsafe {
        core::ptr::copy_nonoverlapping(name.as_ptr(), out, n);
    }
}
