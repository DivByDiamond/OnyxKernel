//! PTY (pseudo-terminal) pairs — kernel core.
//!
//! A PTY is a bidirectional byte pipeline: the master side is held by the
//! terminal emulator, the slave side looks like a terminal to the program.
//! Bytes written to the master are readable on the slave (m2s ring) and
//! vice versa (s2m ring). This module owns the pair table and lifetime;
//! ring plumbing lives in `ring`, I/O + blocking policy in `stream`, and
//! the /dev/ptmx + /dev/pts/N node wiring in devfs::pty_nodes.
//!
//! Lifetime: opening /dev/ptmx allocates a pair; closing the master fd
//! frees it (slave fds then fail with EPIPE, mirroring Linux). The pair
//! size is deliberately small (PTY_MAX x 2 x PTY_BUF_CAP bytes of BSS).

use crate::sync::SpinLock;
use onyx_core::errno::{Errno, KResult};

mod ring;

pub use ring::PtyRing;
pub mod stream;

pub use stream::{side_poll, side_read, side_write};

/// Number of concurrent PTY pairs.
pub const PTY_MAX: usize = 4;
/// Per-direction ring capacity.
pub const PTY_BUF_CAP: usize = 512;

/// Terminal window size carried with the pair (TIOCGWINSZ/TIOCSWINSZ).
#[derive(Clone, Copy)]
pub struct PtyWinsize {
    pub rows: u16,
    pub cols: u16,
    pub xpixel: u16,
    pub ypixel: u16,
}

impl PtyWinsize {
    pub const fn default_80x24() -> Self {
        PtyWinsize {
            rows: 24,
            cols: 80,
            xpixel: 0,
            ypixel: 0,
        }
    }
}

pub struct Pty {
    /// Per-pair spinlock (same discipline as IPC channels: never held
    /// across sched_yield).
    pub lock: SpinLock,
    pub used: bool,
    /// master -> slave direction.
    pub m2s: PtyRing,
    /// slave -> master direction.
    pub s2m: PtyRing,
    pub ws: PtyWinsize,
}

impl Pty {
    pub const fn new() -> Self {
        Pty {
            lock: SpinLock::new(),
            used: false,
            m2s: PtyRing::new(),
            s2m: PtyRing::new(),
            ws: PtyWinsize::default_80x24(),
        }
    }
}

impl Default for Pty {
    fn default() -> Self {
        Self::new()
    }
}

pub static mut G_PTYS: [Pty; PTY_MAX] = [const { Pty::new() }; PTY_MAX];

/// Claim a free pair. Err(Again) when all PTY_MAX pairs are in use (caller
/// should surface "out of PTYs"; retry later is the sensible reaction).
pub fn alloc() -> KResult<u32> {
    // SAFETY: G_PTYS is a kernel-lifetime static; the loop scans each pair
    // once; SIE=0 excludes same-hart preemption between check and set, and
    // only the allocating context touches a fresh pair here.
    unsafe {
        let base = &raw mut G_PTYS;
        for (i, slot) in (*base).iter_mut().enumerate() {
            if !slot.used {
                slot.used = true;
                slot.m2s = PtyRing::new();
                slot.s2m = PtyRing::new();
                slot.ws = PtyWinsize::default_80x24();
                return Ok(i as u32);
            }
        }
    }
    Err(Errno::Again)
}

/// Release a pair (master close). Blocked readers/writers observe
/// used == false on their next yield-loop pass and fail with EPIPE.
pub fn free(idx: u32) {
    if (idx as usize) < PTY_MAX {
        // SAFETY: idx bounds-checked above; the used flag is the single
        // liveness gate every other operation re-checks under the lock.
        unsafe {
            G_PTYS[idx as usize].lock.lock();
            G_PTYS[idx as usize].used = false;
            G_PTYS[idx as usize].lock.unlock();
        }
    }
}

/// # Safety
///
/// idx must name a pair; the caller runs in a syscall context on this hart.
pub unsafe fn is_used(idx: u32) -> bool {
    // SAFETY: idx bounds-checked; plain bool load under the pair lock.
    unsafe {
        if (idx as usize) >= PTY_MAX {
            return false;
        }
        let p = &raw mut G_PTYS[idx as usize];
        (*p).lock.lock();
        let used = (*p).used;
        (*p).lock.unlock();
        used
    }
}

/// Read the pair's window size (TIOCGWINSZ on either side).
///
/// # Safety
///
/// idx bounds-checked inside; syscall context only.
pub unsafe fn winsize(idx: u32) -> PtyWinsize {
    // SAFETY: idx bounds-checked; field read under the pair lock.
    unsafe {
        let p = &raw mut G_PTYS[idx as usize];
        (*p).lock.lock();
        let ws = (*p).ws;
        (*p).lock.unlock();
        ws
    }
}

/// Update the pair's window size (TIOCSWINSZ on either side).
///
/// # Safety
///
/// idx bounds-checked inside; syscall context only.
pub unsafe fn set_winsize(idx: u32, ws: PtyWinsize) {
    // SAFETY: idx bounds-checked; field write under the pair lock.
    unsafe {
        let p = &raw mut G_PTYS[idx as usize];
        (*p).lock.lock();
        (*p).ws = ws;
        (*p).lock.unlock();
    }
}

#[cfg(test)]
mod tests;
