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
pub const DEVFS_HVC0_INO: u32 = 49;
const DEVFS_EXTRA_FB_BASE_INO: u32 = 64;

pub fn fb_ino(index: usize) -> u32 {
    if index == 0 { DEVFS_FB0_INO } else { DEVFS_EXTRA_FB_BASE_INO + index as u32 - 1 }
}

pub fn is_framebuffer_ino(ino: u32) -> bool { framebuffer(ino).is_some() }

fn framebuffer(ino: u32) -> Option<fb::Framebuffer> {
    let index = if ino == DEVFS_FB0_INO { 0 } else {
        ino.checked_sub(DEVFS_EXTRA_FB_BASE_INO)? as usize + 1
    };
    fb::display(index)
}

fn fb_index(name: &[u8]) -> Option<usize> {
    let digits = name.strip_prefix(b"fb")?;
    if digits.is_empty() { return None; }
    let mut index = 0usize;
    for &c in digits {
        if !c.is_ascii_digit() { return None; }
        index = index.checked_mul(10)?.checked_add((c - b'0') as usize)?;
    }
    (index < fb::count()).then_some(index)
}

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
    if name == b"hvc0" && crate::drivers::virtio_console::present() { return Ok(DEVFS_HVC0_INO); }
    if let Some(index) = fb_index(name) { return Ok(fb_ino(index)); }
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
        DEVFS_HVC0_INO if crate::drivers::virtio_console::present() => Ok(st(ino, u32::MAX, 0o100600)),
        ino if is_framebuffer_ino(ino) => Ok(st(ino, framebuffer(ino).unwrap().size as u32, 0o100666)),
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

pub fn readdir_entry(idx: u32, name_out: *mut u8, name_len: usize) -> Option<u32> {
    if idx < 2 {
        copy_name(if idx == 0 { b"." } else { b".." }, name_out, name_len);
        return Some(DEVFS_ROOT_INO);
    }
    let index = (idx - 2) as usize;
    if index < fb::count() {
        let mut name = [0u8; 4];
        name[..2].copy_from_slice(b"fb");
        let length = if index < 10 { name[2] = b'0' + index as u8; 3 }
            else { name[2] = b'0' + (index / 10) as u8; name[3] = b'0' + (index % 10) as u8; 4 };
        copy_name(&name[..length], name_out, name_len);
        return Some(fb_ino(index));
    }
    let dev_idx = index - fb::count();
    if dev_idx < crate::drivers::virtio::count() {
        copy_name(blk::name(dev_idx), name_out, name_len);
        return Some(blk_ino(dev_idx));
    }
    if crate::drivers::virtio_console::present() {
        if dev_idx == crate::drivers::virtio::count() {
            copy_name(b"hvc0", name_out, name_len);
            return Some(DEVFS_HVC0_INO);
        }
        pty_nodes::readdir_entry(idx - 1, name_out, name_len)
    } else {
        pty_nodes::readdir_entry(idx, name_out, name_len)
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

fn copy_name(name: &[u8], out: *mut u8, max_len: usize) {
    let n = name.len().min(max_len);
    // SAFETY: n <= max_len by construction, so the copy stays within the
    // caller-provided name_out buffer of max_len bytes (no NUL is written
    // by this helper).
    unsafe {
        ptr::copy_nonoverlapping(name.as_ptr(), out, n);
    }
}
