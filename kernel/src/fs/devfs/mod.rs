use crate::drivers::fb;
use core::ptr;
use onyx_core::errno::{Errno, KResult};

mod blk;
mod pty_ioctl;
mod pty_nodes;

pub use blk::{blk_ino, is_blk_ino};
pub use pty_ioctl::pty_ioctl;
pub use pty_ioctl::{TIOCGPTN, TIOCGWINSZ, TIOCSWINSZ};
pub use pty_nodes::{DEVFS_PTMX_INO, is_pty_ino, pty_poll, ptym_ino, ptym_ino_idx, ptys_ino_idx};

pub const DEVFS_ROOT_INO: u32 = 1;
pub const DEVFS_FB0_INO: u32 = 2;

/// Base inode for block devices: /dev/blk<N> has inode DEVFS_BLK_BASE_INO + N.
/// Chosen to avoid collision with root (1), fb0 (2) and legacy blk0 (3).
pub const DEVFS_BLK_BASE_INO: u32 = 16;

pub struct DevfsStat {
    pub ino: u32,
    pub size: u32,
    pub mode: u32,
}

pub fn lookup(name: &[u8]) -> KResult<u32> {
    if name.is_empty() || name == b"." {
        return Ok(DEVFS_ROOT_INO);
    }
    if let Some(ino) = pty_nodes::lookup_name(name) {
        return Ok(ino);
    }
    if name == b"fb0" {
        return Ok(DEVFS_FB0_INO);
    }
    if let Some(idx) = blk::parse_name(name) {
        if idx < crate::drivers::virtio::count() {
            return Ok(blk_ino(idx));
        }
        return Err(Errno::NoEnt);
    }
    Err(Errno::NoEnt)
}

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
        DEVFS_ROOT_INO => Ok(st(ino, 0, 0o040755)),
        DEVFS_FB0_INO => Ok(st(ino, fb::size_bytes() as u32, 0o100666)),
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
        match ino {
            DEVFS_FB0_INO => {
                let size = fb::size_bytes() as u32;
                let to_read = len.min(size.saturating_sub(offset));
                if to_read == 0 {
                    return Ok(0);
                }
                let fb_base = fb::fb_base_ptr();
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
            DEVFS_FB0_INO => {
                let size = fb::size_bytes() as u32;
                let to_write = len.min(size.saturating_sub(offset));
                if to_write == 0 {
                    return Ok(0);
                }
                let fb_base = fb::fb_base_ptr();
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

pub fn readdir_entry(idx: u32, name_out: *mut u8, name_len: usize) -> Option<u32> {
    match idx {
        0 => {
            copy_name(b".", name_out, name_len);
            Some(DEVFS_ROOT_INO)
        }
        1 => {
            copy_name(b"..", name_out, name_len);
            Some(DEVFS_ROOT_INO)
        }
        2 => {
            copy_name(b"fb0", name_out, name_len);
            Some(DEVFS_FB0_INO)
        }
        i => {
            let n_devs = crate::drivers::virtio::count();
            let dev_idx = (i - 3) as usize;
            if dev_idx < n_devs {
                copy_name(blk::name(dev_idx), name_out, name_len);
                return Some(blk_ino(dev_idx));
            }
            // PTY nodes live right after the block devices (ptymx + pts/N).
            pty_nodes::readdir_entry(i, name_out, name_len)
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
            DEVFS_FB0_INO => {
                let fb_pa = fb::fb_base_pa();
                let fb_size = fb::size_bytes() as u64;
                let map_len = length.min(fb_size);
                // SAFETY: maps the device's physical fb range (fb_base_pa,
                // bounded by map_len <= fb_size) into the current process's
                // own address space; vaddr validity is established by the
                // mmap syscall path (resolve_vaddr) before dispatch.
                let p = crate::proc::current();
                let flags = pte_flags | crate::arch::regs::PTE_A | crate::arch::regs::PTE_D;
                crate::mm::vmm::map(p.root_pa, vaddr, fb_pa as u64, map_len as usize, flags)?;
                Ok(vaddr)
            }
            _ => Err(Errno::NoSys),
        }
    }
}

pub const FB_IOCTL_GET_INFO: u64 = 0x4600;

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
            DEVFS_FB0_INO => match request {
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
                    let dst = pa as *mut u32;
                    *dst = fb::width() as u32;
                    *dst.add(1) = fb::height() as u32;
                    *dst.add(2) = fb::bpp() as u32;
                    *dst.add(3) = fb::pitch() as u32;
                    *dst.add(4) = fb::size_bytes() as u32;
                    Ok(0)
                }
                _ => Err(Errno::NoSys),
            },
            _ => Err(Errno::NoSys),
        }
    }
}

fn copy_name(name: &[u8], out: *mut u8, max_len: usize) {
    let n = name.len().min(max_len);
    // SAFETY: n <= max_len by construction, so the copy stays within the
    // caller-provided name_out buffer of max_len bytes (no NUL is written
    // by this helper).
    unsafe {
        ptr::copy_nonoverlapping(name.as_ptr(), out, n);
    }
}
