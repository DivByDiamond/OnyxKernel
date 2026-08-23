use crate::drivers::fb;
use core::ptr;
use onyx_core::errno::{Errno, KResult};

mod blk;

pub use blk::{blk_ino, is_blk_ino};

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

pub fn stat(ino: u32) -> KResult<DevfsStat> {
    if let Some(idx) = is_blk_ino(ino) {
        if idx >= crate::drivers::virtio::count() {
            return Err(Errno::NoEnt);
        }
        return Ok(DevfsStat {
            ino,
            size: u32::MAX,
            mode: 0o100666,
        });
    }
    match ino {
        DEVFS_ROOT_INO => Ok(DevfsStat {
            ino,
            size: 0,
            mode: 0o040755,
        }),
        DEVFS_FB0_INO => Ok(DevfsStat {
            ino,
            size: fb::size_bytes() as u32,
            mode: 0o100666,
        }),
        _ => Err(Errno::NoEnt),
    }
}

pub unsafe fn read(ino: u32, buf: *mut u8, offset: u32, len: u32) -> KResult<u32> { unsafe {
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
            ptr::copy_nonoverlapping(fb_base.add(offset as usize), buf, to_read as usize);
            Ok(to_read)
        }
        _ => Err(Errno::NoSys),
    }
}}

pub unsafe fn write(ino: u32, buf: *const u8, offset: u32, len: u32) -> KResult<u32> { unsafe {
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
            ptr::copy_nonoverlapping(buf, fb_base.add(offset as usize), to_write as usize);
            Ok(to_write)
        }
        _ => Err(Errno::NoSys),
    }
}}

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
            let dev_idx = (i - 3) as usize;
            if dev_idx >= crate::drivers::virtio::count() {
                return None;
            }
            copy_name(blk::name(dev_idx), name_out, name_len);
            Some(blk_ino(dev_idx))
        }
    }
}

pub unsafe fn mmap(ino: u32, vaddr: u64, length: u64, pte_flags: u64) -> KResult<u64> { unsafe {
    match ino {
        DEVFS_FB0_INO => {
            let fb_pa = fb::fb_base_pa();
            let fb_size = fb::size_bytes() as u64;
            let map_len = length.min(fb_size);
            let p = crate::proc::current();
            let flags = pte_flags | crate::arch::regs::PTE_A | crate::arch::regs::PTE_D;
            crate::mm::vmm::map(p.root_pa, vaddr, fb_pa as u64, map_len as usize, flags)?;
            Ok(vaddr)
        }
        _ => Err(Errno::NoSys),
    }
}}

pub const FB_IOCTL_GET_INFO: u64 = 0x4600;

pub unsafe fn ioctl(ino: u32, request: u64, arg: u64) -> KResult<i64> { unsafe {
    match ino {
        DEVFS_FB0_INO => match request {
            FB_IOCTL_GET_INFO => {
                let p = crate::proc::current();
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
}}

fn copy_name(name: &[u8], out: *mut u8, max_len: usize) {
    let n = name.len().min(max_len);
    unsafe {
        ptr::copy_nonoverlapping(name.as_ptr(), out, n);
    }
}
