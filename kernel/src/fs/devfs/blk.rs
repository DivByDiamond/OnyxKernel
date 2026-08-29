//! Block device nodes /dev/blk0../dev/blk7, one per probed virtio-blk device.
use crate::drivers::{virtio, virtio_req};
use onyx_core::errno::{Errno, KResult};

use super::DEVFS_BLK_BASE_INO;

#[inline]
pub const fn blk_ino(idx: usize) -> u32 {
    DEVFS_BLK_BASE_INO + idx as u32
}

#[inline]
pub const fn is_blk_ino(ino: u32) -> Option<usize> {
    let last = DEVFS_BLK_BASE_INO + virtio::VIRTIO_MAX_DEVS as u32;
    if ino >= DEVFS_BLK_BASE_INO && ino < last {
        Some((ino - DEVFS_BLK_BASE_INO) as usize)
    } else {
        None
    }
}

pub fn parse_name(name: &[u8]) -> Option<usize> {
    let digits = name.strip_prefix(b"blk")?;
    if digits.is_empty() || !digits.iter().all(|d| d.is_ascii_digit()) {
        return None;
    }
    let mut n: usize = 0;
    for &d in digits {
        n = n.checked_mul(10)?.checked_add((d - b'0') as usize)?;
        if n >= virtio::VIRTIO_MAX_DEVS {
            return None;
        }
    }
    Some(n)
}

/// # Safety
///
/// Caller contract: `buf` must be writable for `len` bytes - for user
/// callers the syscall layer validated and translated the range
/// (user_ptr_ok/check_user_range); dev_idx is re-checked inside against
/// virtio::count().
pub unsafe fn read(dev_idx: usize, buf: *mut u8, offset: u32, len: u32) -> KResult<u32> {
    if dev_idx >= virtio::count() {
        return Err(Errno::NoEnt);
    }
    let Some((lba, n_sectors)) = sector_range(offset, len)? else {
        return Ok(0);
    };
    // SAFETY: dev_idx is bounds-checked above (< virtio::count()); buf
    // covers n_sectors * 512 bytes per the read_multi contract.
    unsafe { virtio_req::read_multi(dev_idx, lba, n_sectors, buf)? };
    Ok(n_sectors * 512)
}

/// # Safety
///
/// Caller contract: `buf` must be readable for `len` bytes - for user
/// callers the syscall layer validated and translated the range (user_ptr_ok);
/// dev_idx is re-checked inside against virtio::count().
pub unsafe fn write(dev_idx: usize, buf: *const u8, offset: u32, len: u32) -> KResult<u32> {
    if dev_idx >= virtio::count() {
        return Err(Errno::NoEnt);
    }
    let Some((lba, n_sectors)) = sector_range(offset, len)? else {
        return Ok(0);
    };
    // SAFETY: dev_idx is bounds-checked above (< virtio::count()); buf
    // covers n_sectors * 512 bytes per the write_multi contract.
    unsafe { virtio_req::write_multi(dev_idx, lba, n_sectors, buf)? };
    Ok(n_sectors * 512)
}

/// Split a byte offset/length into an LBA and sector count.
/// Returns `Ok(None)` when the length covers fewer than one sector.
fn sector_range(offset: u32, len: u32) -> KResult<Option<(u64, u32)>> {
    if !offset.is_multiple_of(512) {
        return Err(Errno::Inval);
    }
    let n_sectors = len / 512;
    if n_sectors == 0 {
        return Ok(None);
    }
    Ok(Some(((offset / 512) as u64, n_sectors)))
}

pub const fn name(dev_idx: usize) -> &'static [u8] {
    match dev_idx {
        0 => b"blk0",
        1 => b"blk1",
        2 => b"blk2",
        3 => b"blk3",
        4 => b"blk4",
        5 => b"blk5",
        6 => b"blk6",
        7 => b"blk7",
        _ => b"blk?",
    }
}
