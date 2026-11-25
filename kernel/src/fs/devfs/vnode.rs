//! Vnode operations on devfs nodes: stat, read/write, ioctl and mmap.

use core::ptr;
use onyx_core::errno::{Errno, KResult};

use super::pty::pty_ioctl::pty_ioctl;
use super::pty::pty_nodes;
use super::{
    DEVFS_FB0_INO, DEVFS_HVC0_INO, DevfsStat, blk, framebuffer, is_blk_ino, is_framebuffer_ino,
    is_js_ino, js_read,
};

fn st(ino: u32, size: u32, mode: u32) -> DevfsStat {
    DevfsStat { ino, size, mode }
}

pub fn stat(ino: u32) -> KResult<DevfsStat> {
    if let Some((ino, size)) = pty_nodes::stat(ino) {
        return Ok(st(ino, size, 0o100666));
    }
    if let Some(idx) = is_blk_ino(ino) {
        if idx >= crate::drivers::virtio::count() {
            return Err(Errno::NoEnt);
        }
        return Ok(st(ino, u32::MAX, 0o100666));
    }
    match ino {
        super::DEVFS_ROOT_INO => Ok(st(ino, 0, 0o040755)),
        DEVFS_HVC0_INO if crate::drivers::virtio_console::present() => {
            Ok(st(ino, u32::MAX, 0o100600))
        }
        // js before fb: a js inode must never resolve as a framebuffer
        // (they cannot collide at plausible display counts, but a js
        // read misrouted into fb memory would be far worse than the
        // reverse). u32::MAX size keeps the positional fd layer from
        // truncating ring reads (the pty/block trick).
        ino if is_js_ino(ino).is_some() => Ok(st(ino, u32::MAX, 0o100600)),
        ino if is_framebuffer_ino(ino) => {
            Ok(st(ino, framebuffer(ino).unwrap().size as u32, 0o100666))
        }
        _ => Err(Errno::NoEnt),
    }
}

/// # Safety
///
/// Caller contract: `buf` must be writable for `len` bytes (validated and
/// translated by the syscall layer for user callers); ino must come from a
/// devfs lookup/stat.
pub unsafe fn read(ino: u32, buf: *mut u8, offset: u32, len: u32) -> KResult<u32> {
    // SAFETY: buffer contract documented on the fn; per-branch bounds noted below.
    unsafe {
        if pty_nodes::is_pty_ino(ino) {
            // SAFETY: pty_nodes::read re-checks pair liveness under its lock.
            return pty_nodes::read(ino, buf, offset, len);
        }
        if let Some(idx) = is_blk_ino(ino) {
            // SAFETY: caller guarantees buf points to a writable region of at
            // least `len` bytes; blk::read validates the device index.
            return blk::read(idx, buf, offset, len);
        }
        if let Some(idx) = is_js_ino(ino) {
            // SAFETY: buf covers len bytes per the caller contract;
            // js_read re-checks the slot bounds under its ring lock.
            // Records are non-positional — the fd offset is ignored
            // (the pty read contract).
            return js_read(idx, buf, len);
        }
        match ino {
            DEVFS_HVC0_INO => crate::drivers::virtio_console::read(buf, len as usize),
            ino if is_framebuffer_ino(ino) => {
                let display = framebuffer(ino).ok_or(Errno::NoEnt)?;
                let size = display.size as u32;
                let to_read = len.min(size.saturating_sub(offset));
                if to_read == 0 {
                    return Ok(0);
                }
                let fb_base = display.base as *mut u8;
                // SAFETY: to_read = min(len, size - offset) after the
                // early-return, so offset + to_read <= fb size; buf covers
                // `len` bytes per the caller contract, hence to_read too.
                ptr::copy_nonoverlapping(fb_base.add(offset as usize), buf, to_read as usize);
                Ok(to_read)
            }
            _ => Err(Errno::NoSys),
        }
    }
}

/// # Safety
///
/// Caller contract: `buf` must be readable for `len` bytes (validated and
/// translated by the syscall layer for user callers); ino must come from a
/// devfs lookup/stat.
pub unsafe fn write(ino: u32, buf: *const u8, offset: u32, len: u32) -> KResult<u32> {
    // SAFETY: buffer contract documented on the fn; per-branch bounds checked.
    unsafe {
        if pty_nodes::is_pty_ino(ino) {
            // SAFETY: pty_nodes::write re-checks pair liveness under its lock.
            return pty_nodes::write(ino, buf, offset, len);
        }
        if let Some(idx) = is_blk_ino(ino) {
            // SAFETY: caller guarantees buf points to a readable region of at
            // least `len` bytes; blk::write validates the device index.
            return blk::write(idx, buf, offset, len);
        }
        match ino {
            DEVFS_HVC0_INO => crate::drivers::virtio_console::write(buf, len as usize),
            ino if is_framebuffer_ino(ino) => {
                let display = framebuffer(ino).ok_or(Errno::NoEnt)?;
                let size = display.size as u32;
                let to_write = len.min(size.saturating_sub(offset));
                if to_write == 0 {
                    return Ok(0);
                }
                let fb_base = display.base as *mut u8;
                // SAFETY: to_write = min(len, size - offset) after the
                // early-return, so offset + to_write <= fb size; buf covers
                // `len` readable bytes per the caller contract.
                ptr::copy_nonoverlapping(buf, fb_base.add(offset as usize), to_write as usize);
                Ok(to_write)
            }
            _ => Err(Errno::NoSys),
        }
    }
}

/// # Safety
///
/// Caller contract: vaddr/length/pte_flags come from the mmap syscall after
/// address-space validation (resolve_vaddr); runs in the mmap'ing process's
/// syscall context, so p.root_pa is its own page table.
pub unsafe fn mmap(ino: u32, vaddr: u64, length: u64, pte_flags: u64) -> KResult<u64> {
    // SAFETY: mapping scoped to the caller's own page table; see # Safety.
    unsafe {
        match ino {
            ino if is_framebuffer_ino(ino) => {
                let display = framebuffer(ino).ok_or(Errno::NoEnt)?;
                let fb_pa = display.base;
                let fb_size = display.size as u64;
                let offset = fb_pa as u64 & 0xFFF;
                let aligned_pa = fb_pa as u64 - offset;
                let map_len = length.min(fb_size) + offset;
                // SAFETY: map the covering physical pages; the returned
                // pointer skips the prefix and exposes the requested display
                // bytes. resolve_vaddr reserves the full rounded extent in
                // the current process's address space before dispatch.
                let p = crate::proc::current();
                let flags = pte_flags | crate::arch::regs::PTE_A | crate::arch::regs::PTE_D;
                crate::mm::vmm::map(p.root_pa, vaddr, aligned_pa, map_len as usize, flags)?;
                Ok(vaddr + offset)
            }
            _ => Err(Errno::NoSys),
        }
    }
}

/// Prefix needed when exposing a packed, non-page-aligned MMIO surface.
pub fn mmap_prefix(ino: u32) -> u64 {
    framebuffer(ino).map_or(0, |display| display.base as u64 & 0xFFF)
}

pub const FB_IOCTL_GET_INFO: u64 = 0x4600;
pub const FB_IOCTL_CLAIM_GRAPHICS: u64 = 0x46F0;
pub const FB_IOCTL_RELEASE_GRAPHICS: u64 = 0x46F1;
/// Register a hotkey chord (arg packs id/mods/code, see `input::hotkey`);
/// claim owner only.
pub const FB_IOCTL_HOTKEY_SET: u64 = 0x46F2;
/// Pop the next pending hotkey id (0 = none); claim owner only.
pub const FB_IOCTL_HOTKEY_POLL: u64 = 0x46F3;
/// Drop every registered hotkey; claim owner only.
pub const FB_IOCTL_HOTKEY_CLEAR: u64 = 0x46F4;

/// The three hotkey requests are valid only for the process that holds
/// the display claim: the desktop daemon owns both the screen and the
/// global key chords.
fn hotkey_ioctl(pid: u32, request: u64, arg: u64) -> KResult<i64> {
    if crate::drivers::fb_term::graphics::owner() != pid {
        return Err(Errno::Perm);
    }
    match request {
        FB_IOCTL_HOTKEY_SET => crate::drivers::input::hotkey::register(pid, arg).map(|()| 0),
        FB_IOCTL_HOTKEY_POLL => crate::drivers::input::hotkey::poll(pid).map(i64::from),
        _ => {
            crate::drivers::input::hotkey::clear(pid);
            Ok(0)
        }
    }
}

/// # Safety
///
/// Caller contract: for FB_IOCTL_GET_INFO the syscall layer (sys_ioctl) has
/// validated `arg` as a writable user range of 20 bytes via check_user_range
/// before dispatching; arg must be page-aligned to a mapped user page.
pub unsafe fn ioctl(ino: u32, request: u64, arg: u64) -> KResult<i64> {
    // SAFETY: user-pointer contract for `arg` upheld by sys_ioctl; see below.
    unsafe {
        if pty_nodes::is_pty_ino(ino) {
            // SAFETY: winsize/ptn writes go through the 8-byte user window
            // validated by sys_ioctl (same contract as FB_IOCTL_GET_INFO).
            return pty_ioctl(ino, request, arg);
        }
        match ino {
            ino if is_framebuffer_ino(ino) => match request {
                FB_IOCTL_CLAIM_GRAPHICS if ino == DEVFS_FB0_INO => {
                    crate::drivers::fb_term::graphics::claim(crate::proc::current().pid)?;
                    Ok(0)
                }
                FB_IOCTL_RELEASE_GRAPHICS if ino == DEVFS_FB0_INO => {
                    if crate::drivers::fb_term::graphics::release(crate::proc::current().pid) {
                        Ok(0)
                    } else {
                        Err(Errno::Perm)
                    }
                }
                FB_IOCTL_HOTKEY_SET | FB_IOCTL_HOTKEY_POLL | FB_IOCTL_HOTKEY_CLEAR
                    if ino == DEVFS_FB0_INO =>
                {
                    // SAFETY: syscall context, current() is the caller.
                    hotkey_ioctl(crate::proc::current().pid, request, arg)
                }
                FB_IOCTL_GET_INFO => {
                    let p = crate::proc::current();
                    // SAFETY: sys_ioctl pre-validated arg as a writable user
                    // range of 20 bytes (check_user_range); translate()
                    // re-walks the page table and 0 here means unmapped,
                    // which we reject. The 5-u32 store stays within the
                    // validated range.
                    let pa = crate::mm::vmm::translate(p.root_pa, arg);
                    if pa == 0 {
                        return Err(Errno::Inval);
                    }
                    let display = framebuffer(ino).ok_or(Errno::NoEnt)?;
                    let dst = pa as *mut u32;
                    *dst = display.width as u32;
                    *dst.add(1) = display.height as u32;
                    *dst.add(2) = display.bpp as u32;
                    *dst.add(3) = display.pitch as u32;
                    *dst.add(4) = display.size as u32;
                    Ok(0)
                }
                _ => Err(Errno::NoSys),
            },
            _ => Err(Errno::NoSys),
        }
    }
}
