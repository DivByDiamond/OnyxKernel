//! PTY ioctl hooks in devfs: TIOCGPTN (pts number), TIOCGWINSZ /
//! TIOCSWINSZ (window size). The request numbers are Linux-compatible.
//! User-memory access goes through the caller-validated window that
//! sys_ioctl pre-checks (direction depends on the request).

use super::pty_nodes::{ptym_ino_idx, ptys_ino_idx};
use crate::fs::pty;
use onyx_core::errno::{Errno, KResult};

/// Linux-compatible TIOCGPTN: report the pts number on a pair fd.
pub const TIOCGPTN: u64 = 0x8004_5430;
/// TIOCGWINSZ/TIOCSWINSZ (Linux numbers, 8-byte struct winsize).
pub const TIOCGWINSZ: u64 = 0x5413;
pub const TIOCSWINSZ: u64 = 0x5414;

/// devfs::ioctl hook: TIOCGPTN on the master, winsize get/set on either.
///
/// # Safety
///
/// For the request paths that write to `arg` the syscall layer must have
/// validated it as a writable user range of 8 bytes before dispatch (the
/// same contract as the fb GET_INFO ioctl).
pub unsafe fn pty_ioctl(ino: u32, request: u64, arg: u64) -> KResult<i64> {
    // SAFETY: the only user-memory write (winsize out / pt number out) is
    // guarded by the caller-validated 8-byte window translated below.
    unsafe {
        let idx = match ptym_ino_idx(ino) {
            Some(i) => i,
            None => ptys_ino_idx(ino).ok_or(Errno::NoEnt)?,
        };
        match request {
            TIOCGPTN => {
                // The pts number is meaningful on either side of the pair.
                write_u32(arg, idx)
            }
            TIOCGWINSZ => {
                let ws = pty::winsize(idx);
                write_winsize(arg, ws)
            }
            TIOCSWINSZ => {
                let ws = read_winsize(arg)?;
                pty::set_winsize(idx, ws);
                Ok(0)
            }
            _ => Err(Errno::NoSys),
        }
    }
}

/// # Safety
///
/// arg must be a validated writable 8-byte user window (caller contract).
unsafe fn write_winsize(arg: u64, ws: pty::PtyWinsize) -> KResult<i64> {
    // SAFETY: arg validated by the syscall layer per the fn contract; the
    // translation is re-walked and zero-rejected before any store.
    unsafe {
        if arg == 0 {
            return Ok(0);
        }
        let pa = crate::mm::vmm::translate(crate::proc::current().root_pa, arg);
        if pa == 0 {
            return Err(Errno::Inval);
        }
        let w = pa as *mut u16;
        core::ptr::write(w.add(0), ws.rows);
        core::ptr::write(w.add(1), ws.cols);
        core::ptr::write(w.add(2), ws.xpixel);
        core::ptr::write(w.add(3), ws.ypixel);
        Ok(0)
    }
}

/// # Safety
///
/// arg must be a validated readable 8-byte user window (caller contract).
unsafe fn read_winsize(arg: u64) -> KResult<pty::PtyWinsize> {
    // SAFETY: arg validated by the syscall layer per the fn contract; the
    // translation is re-walked and zero-rejected before any load.
    unsafe {
        if arg == 0 {
            return Err(Errno::Inval);
        }
        let pa = crate::mm::vmm::translate(crate::proc::current().root_pa, arg);
        if pa == 0 {
            return Err(Errno::Inval);
        }
        let r = pa as *const u16;
        Ok(pty::PtyWinsize {
            rows: core::ptr::read(r.add(0)),
            cols: core::ptr::read(r.add(1)),
            xpixel: core::ptr::read(r.add(2)),
            ypixel: core::ptr::read(r.add(3)),
        })
    }
}

/// # Safety
///
/// arg must be a validated writable 4-byte user window (caller contract).
unsafe fn write_u32(arg: u64, v: u32) -> KResult<i64> {
    // SAFETY: arg validated by the syscall layer per the fn contract; the
    // translation is re-walked and zero-rejected before the store.
    unsafe {
        if arg == 0 {
            return Ok(0);
        }
        let pa = crate::mm::vmm::translate(crate::proc::current().root_pa, arg);
        if pa == 0 {
            return Err(Errno::Inval);
        }
        core::ptr::write(pa as *mut u32, v);
        Ok(0)
    }
}
