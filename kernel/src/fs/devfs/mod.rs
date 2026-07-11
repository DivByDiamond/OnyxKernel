use crate::drivers::fb;
use crate::drivers::virtio_req;
use core::ptr;
use onyx_core::errno::{Errno, KResult};

pub const DEVFS_ROOT_INO: u32 = 1;
pub const DEVFS_FB0_INO: u32 = 2;
pub const DEVFS_BLK0_INO: u32 = 3;

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
    if name == b"blk0" {
        return Ok(DEVFS_BLK0_INO);
    }
    Err(Errno::NoEnt)
}

pub fn stat(ino: u32) -> KResult<DevfsStat> {
    match ino {
        DEVFS_ROOT_INO => Ok(DevfsStat {
            ino,
            size: 0,
            mode: 0o040755,
        }),
        DEVFS_FB0_INO => Ok(DevfsStat {
            ino,
            size: fb::FB_SIZE as u32,
            mode: 0o100666,
        }),
        DEVFS_BLK0_INO => Ok(DevfsStat {
            ino,
            size: u32::MAX,
            mode: 0o100666,
        }),
        _ => Err(Errno::NoEnt),
    }
}

pub unsafe fn read(ino: u32, buf: *mut u8, offset: u32, len: u32) -> KResult<u32> {
    match ino {
        DEVFS_FB0_INO => {
            let size = fb::FB_SIZE as u32;
            let to_read = len.min(size.saturating_sub(offset));
            if to_read == 0 {
                return Ok(0);
            }
            let fb_base = fb::fb_base_ptr();
            ptr::copy_nonoverlapping(fb_base.add(offset as usize), buf, to_read as usize);
            Ok(to_read)
        }
        DEVFS_BLK0_INO => {
            if offset % 512 != 0 {
                return Err(Errno::Inval);
            }
            let lba = offset / 512;
            let n_sectors = len / 512;
            if n_sectors == 0 {
                return Ok(0);
            }
            virtio_req::read_multi(0, lba as u64, n_sectors, buf)?;
            Ok(n_sectors * 512)
        }
        _ => Err(Errno::NoSys),
    }
}

pub unsafe fn write(ino: u32, buf: *const u8, offset: u32, len: u32) -> KResult<u32> {
    match ino {
        DEVFS_FB0_INO => {
            let size = fb::FB_SIZE as u32;
            let to_write = len.min(size.saturating_sub(offset));
            if to_write == 0 {
                return Ok(0);
            }
            let fb_base = fb::fb_base_ptr();
            ptr::copy_nonoverlapping(buf, fb_base.add(offset as usize), to_write as usize);
            Ok(to_write)
        }
        DEVFS_BLK0_INO => {
            if offset % 512 != 0 {
                return Err(Errno::Inval);
            }
            let lba = offset / 512;
            let n_sectors = len / 512;
            if n_sectors == 0 {
                return Ok(0);
            }
            virtio_req::write_multi(0, lba as u64, n_sectors, buf)?;
            Ok(n_sectors * 512)
        }
        _ => Err(Errno::NoSys),
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
        3 => {
            copy_name(b"blk0", name_out, name_len);
            Some(DEVFS_BLK0_INO)
        }
        _ => None,
    }
}

pub unsafe fn mmap(ino: u32, vaddr: u64, length: u64, pte_flags: u64) -> KResult<u64> {
    match ino {
        DEVFS_FB0_INO => {
            let fb_pa = fb::fb_base_pa();
            let fb_size = fb::FB_SIZE as u64;
            let map_len = length.min(fb_size);
            let p = crate::proc::current();
            let flags = pte_flags | crate::arch::regs::PTE_A | crate::arch::regs::PTE_D;
            crate::mm::vmm::map(p.root_pa, vaddr, fb_pa as u64, map_len as usize, flags)?;
            Ok(vaddr)
        }
        _ => Err(Errno::NoSys),
    }
}

pub const FB_IOCTL_GET_INFO: u64 = 0x4600;

pub unsafe fn ioctl(ino: u32, request: u64, arg: u64) -> KResult<i64> {
    match ino {
        DEVFS_FB0_INO => match request {
            FB_IOCTL_GET_INFO => {
                let p = crate::proc::current();
                let pa = crate::mm::vmm::translate(p.root_pa, arg);
                if pa == 0 {
                    return Err(Errno::Inval);
                }
                let dst = pa as *mut u32;
                *dst = fb::FB_WIDTH as u32;
                *dst.add(1) = fb::FB_HEIGHT as u32;
                *dst.add(2) = fb::FB_BPP as u32;
                *dst.add(3) = fb::FB_PITCH as u32;
                *dst.add(4) = fb::FB_SIZE as u32;
                Ok(0)
            }
            _ => Err(Errno::NoSys),
        },
        DEVFS_BLK0_INO => Err(Errno::NoSys),
        _ => Err(Errno::NoSys),
    }
}

fn copy_name(name: &[u8], out: *mut u8, max_len: usize) {
    let n = name.len().min(max_len);
    unsafe {
        ptr::copy_nonoverlapping(name.as_ptr(), out, n);
    }
}
