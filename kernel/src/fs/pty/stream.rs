//! PTY I/O: per-direction side operations plus the syscall stream hooks.
//!
//! The side_* functions are non-blocking one-pass ring operations under
//! the pair spinlock: Ok(0) means "empty/full, retry later", Err(Pipe)
//! means the pair is gone. The syscall hooks layer the blocking/O_NONBLOCK
//! policy on top with the same sched_yield retry loop as poll(2) and
//! console_read() (the kernel has no sleep-queue for fds). A yield-loop
//! pass is a fresh side op, so a closed pair (EPIPE) is noticed on the
//! very next pass; O_NONBLOCK turns "nothing available" into EAGAIN.

use super::{G_PTYS, PTY_BUF_CAP, PTY_MAX};
use crate::arch::trap_frame::TrapFrame;
use crate::fs::vfs;
use crate::proc;
use crate::syscall::abi::O_NONBLOCK;
use onyx_core::errno::{Errno, KResult};
use onyx_core::ringbuf::ring_free;

/// Low-level, non-blocking read of one direction. Ok(0) means "empty, retry
/// later"; Err(Pipe) means the pair is gone. `from_master` selects s2m (a
/// master read drains what the slave wrote) vs m2s (a slave read).
///
/// # Safety
///
/// `buf` must be writable for `len` bytes (validated by the syscall layer
/// for user callers); idx bounds-checked inside.
pub unsafe fn side_read(idx: u32, from_master: bool, buf: *mut u8, len: u32) -> KResult<u32> {
    // SAFETY: idx bounds-checked before any deref; the ring is only touched
    // under the pair spinlock; buf validity is the caller's contract.
    unsafe {
        if (idx as usize) >= PTY_MAX {
            return Err(Errno::Inval);
        }
        let p = &raw mut G_PTYS[idx as usize];
        (*p).lock.lock();
        if !(*p).used {
            (*p).lock.unlock();
            return Err(Errno::Pipe);
        }
        let ring = if from_master {
            &mut (*p).s2m
        } else {
            &mut (*p).m2s
        };
        if ring.used() == 0 {
            (*p).lock.unlock();
            return Ok(0);
        }
        let n = len.min(ring.used());
        let dst = core::slice::from_raw_parts_mut(buf, n as usize);
        let got = ring.pop(dst);
        (*p).lock.unlock();
        Ok(got)
    }
}

/// Low-level, non-blocking write into one direction. Partial writes are
/// allowed: returns bytes actually stored. Err(Pipe) when the pair is gone
/// (e.g. slave write after master close — like POSIX EPIPE on pty).
///
/// # Safety
///
/// `buf` must be readable for `len` bytes (validated by the syscall layer);
/// idx bounds-checked inside.
pub unsafe fn side_write(idx: u32, to_master: bool, buf: *const u8, len: u32) -> KResult<u32> {
    // SAFETY: idx bounds-checked before any deref; ring mutation happens
    // under the pair spinlock; buf validity is the caller's contract.
    unsafe {
        if (idx as usize) >= PTY_MAX {
            return Err(Errno::Inval);
        }
        let p = &raw mut G_PTYS[idx as usize];
        (*p).lock.lock();
        if !(*p).used {
            (*p).lock.unlock();
            return Err(Errno::Pipe);
        }
        // Writing to the master side feeds m2s (slave reads it); writing to
        // the slave side feeds s2m (master reads it).
        let ring = if to_master {
            &mut (*p).m2s
        } else {
            &mut (*p).s2m
        };
        let free = ring_free(PTY_BUF_CAP, ring.head, ring.tail);
        if free == 0 || len == 0 {
            (*p).lock.unlock();
            return Ok(0);
        }
        let n = len.min(free);
        let src = core::slice::from_raw_parts(buf, n as usize);
        let wrote = ring.push(src);
        (*p).lock.unlock();
        Ok(wrote)
    }
}

/// Readiness snapshot for poll(): (readable, writable) for the given side.
/// A closed pair reports (false, false); poll turns invalid tokens into
/// POLLNVAL before reaching here.
///
/// # Safety
///
/// idx bounds-checked inside; syscall context only.
pub unsafe fn side_poll(idx: u32, master: bool) -> (bool, bool) {
    // SAFETY: idx bounds-checked; the pair lock guards the ring counters.
    unsafe {
        if (idx as usize) >= PTY_MAX {
            return (false, false);
        }
        let p = &raw mut G_PTYS[idx as usize];
        (*p).lock.lock();
        if !(*p).used {
            (*p).lock.unlock();
            return (false, false);
        }
        let (in_ring, out_ring) = if master {
            (&(*p).s2m, &(*p).m2s)
        } else {
            (&(*p).m2s, &(*p).s2m)
        };
        let readable = in_ring.used() > 0;
        let writable = ring_free(PTY_BUF_CAP, out_ring.head, out_ring.tail) > 0;
        (*p).lock.unlock();
        (readable, writable)
    }
}

/// A Devfs ino that routes to the PTY stream path (master or slave node).
/// Returns (pty_idx, is_master) when `ino` belongs to a live pair's fd.
pub fn classify(ino: u32) -> Option<(u32, bool)> {
    if let Some(idx) = crate::fs::devfs::ptym_ino_idx(ino) {
        return Some((idx, true));
    }
    if let Some(idx) = crate::fs::devfs::ptys_ino_idx(ino) {
        return Some((idx, false));
    }
    None
}

/// sys_read hook for PTY fds. Callers verified buf/len with user_buf_ok.
///
/// # Safety
///
/// Syscall path only: `tf` is this hart's live trap frame; `buf` is a
/// validated writable user range of `len` bytes; `fd` is a user fd value.
pub unsafe fn read_hook(tf: &mut TrapFrame, fd: u64, buf: u64, len: u64) -> Option<i64> {
    // SAFETY: fd_check revalidates the user fd token; buf validity is the
    // caller's (user_buf_ok) contract documented above.
    unsafe {
        let idx = match vfs::fd_check(fd) {
            Ok(i) => i,
            Err(e) => return Some(e.as_i64()),
        };
        let f = vfs::fd_get(idx);
        let (pty_idx, master) = classify(f.ino)?;
        let nonblock = f.flags & O_NONBLOCK != 0;
        if len == 0 {
            return Some(0);
        }
        let dst = buf as *mut u8;
        loop {
            match side_read(pty_idx, master, dst, len as u32) {
                Ok(0) => {
                    if nonblock {
                        return Some(Errno::Again.as_i64());
                    }
                    proc::sched_yield(tf);
                }
                Ok(n) => return Some(n as i64),
                Err(e) => return Some(e.as_i64()),
            }
        }
    }
}

/// sys_write hook for PTY fds. Callers verified buf/len with user_buf_ok.
///
/// # Safety
///
/// Syscall path only: `tf` is this hart's live trap frame; `buf` is a
/// validated readable user range of `len` bytes; `fd` is a user fd value.
pub unsafe fn write_hook(tf: &mut TrapFrame, fd: u64, buf: u64, len: u64) -> Option<i64> {
    // SAFETY: fd_check revalidates the user fd token; buf validity is the
    // caller's (user_buf_ok) contract documented above.
    unsafe {
        let idx = match vfs::fd_check(fd) {
            Ok(i) => i,
            Err(e) => return Some(e.as_i64()),
        };
        let f = vfs::fd_get(idx);
        let (pty_idx, master) = classify(f.ino)?;
        let nonblock = f.flags & O_NONBLOCK != 0;
        if len == 0 {
            return Some(0);
        }
        let src = buf as *const u8;
        loop {
            match side_write(pty_idx, master, src, len as u32) {
                // Partial write is a valid result (POSIX allows it); only a
                // completely full ring keeps yielding (or EAGAINs).
                Ok(0) => {
                    if nonblock {
                        return Some(Errno::Again.as_i64());
                    }
                    proc::sched_yield(tf);
                }
                Ok(n) => return Some(n as i64),
                Err(e) => return Some(e.as_i64()),
            }
        }
    }
}
