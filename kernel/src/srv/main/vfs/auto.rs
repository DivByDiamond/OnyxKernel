//! Automatic mounting of additional OC2R block devices under /mnt.

use crate::arch::regs::ONYXFS_LBA;
use crate::fs::fat32;
use crate::fs::onyxfs;
use crate::fs::vfs;
use crate::mm::heap;
use onyx_core::fmt::Arg;

use super::probe;

/// Write decimal digits of `dev` into `out` in normal order. Returns count.
fn write_dev_suffix(dev: usize, out: &mut [u8; 4]) -> Option<usize> {
    let mut rev = [0u8; 4];
    let mut digits = 0usize;
    let mut n = dev;
    if n == 0 {
        rev[0] = b'0';
        digits = 1;
    } else {
        while n > 0 {
            if digits >= 4 {
                return None;
            }
            rev[digits] = b'0' + (n % 10) as u8;
            digits += 1;
            n /= 10;
        }
    }
    out[..digits].copy_from_slice(&rev[..digits]);
    out[..digits].reverse();
    Some(digits)
}

/// Build "/mnt/blkN" into `buf`, returning its length.
fn build_mnt_path(dev: usize, buf: &mut [u8; 16]) -> Option<usize> {
    let mut suffix = [0u8; 4];
    let digits = write_dev_suffix(dev, &mut suffix)?;
    let prefix: &[u8] = b"/mnt/blk";
    let len = prefix.len() + digits;
    buf[..prefix.len()].copy_from_slice(prefix);
    buf[prefix.len()..len].copy_from_slice(&suffix[..digits]);
    Some(len)
}

/// Allocate a leaked 'static path string like "mnt/blk2" for a secondary mount.
unsafe fn alloc_mnt_path(dev: usize) -> Option<&'static [u8]> {
    let mut suffix = [0u8; 4];
    let digits = write_dev_suffix(dev, &mut suffix)?;
    let base: &[u8] = b"mnt/blk";
    let len = base.len() + digits;
    // SAFETY: kmalloc returned `len` writable bytes; all writes stay within
    // the allocation and the resulting slice borrows it for the mount table.
    unsafe {
        let ptr = heap::kmalloc(len).ok()?;
        ptr.copy_from_nonoverlapping(base.as_ptr(), base.len());
        ptr.add(base.len())
            .copy_from_nonoverlapping(suffix.as_ptr(), digits);
        Some(core::slice::from_raw_parts(ptr, len))
    }
}

/// # Safety
///
/// Boot-time secondary mount pass. Runs on the boot hart after the root
/// filesystem has been mounted, before userspace can observe G_MOUNTS.
pub(super) unsafe fn mount_secondary_disks(ndevs: usize, root_dev: Option<usize>) {
    // SAFETY: boot-time VFS setup owns mount-table initialization. Each
    // mount_secondary() call captures a per-volume context and leaves the
    // root singleton state active.
    unsafe {
        let _ = vfs::mkdir(b"/mnt");
        for dev in 0..ndevs {
            if Some(dev) == root_dev {
                continue;
            }
            let (lba, ctx) = if probe::probe_onyxfs(dev, 0) {
                match onyxfs::mount_secondary(dev, 0) {
                    Ok(ctx) => (0u32, vfs::MountFs::Onyx(ctx)),
                    Err(_) => continue,
                }
            } else if probe::probe_onyxfs(dev, ONYXFS_LBA) {
                match onyxfs::mount_secondary(dev, ONYXFS_LBA) {
                    Ok(ctx) => (ONYXFS_LBA, vfs::MountFs::Onyx(ctx)),
                    Err(_) => continue,
                }
            } else if probe::probe_fat32(dev) {
                match fat32::mount_secondary(dev) {
                    Ok(ctx) => (0u32, vfs::MountFs::Fat32(ctx)),
                    Err(_) => continue,
                }
            } else {
                continue;
            };
            let Some(mnt_path) = alloc_mnt_path(dev) else {
                continue;
            };
            let mut abs_path = [0u8; 16];
            let Some(abs_len) = build_mnt_path(dev, &mut abs_path) else {
                continue;
            };
            let _ = vfs::mkdir(&abs_path[..abs_len]);
            if vfs::mount_secondary(mnt_path, dev, lba, ctx).is_ok() {
                crate::kinf!(
                    "vfs",
                    "secondary mounted dev %d at /mnt/blk%d lba %d",
                    Arg::from(dev as u64),
                    Arg::from(dev as u64),
                    Arg::from(lba as u64)
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_mnt_path_for_single_and_multi_digit_devices() {
        let mut buf = [0u8; 16];
        assert_eq!(build_mnt_path(0, &mut buf), Some(9));
        assert_eq!(&buf[..9], b"/mnt/blk0");
        assert_eq!(build_mnt_path(123, &mut buf), Some(11));
        assert_eq!(&buf[..11], b"/mnt/blk123");
    }

    #[test]
    fn rejects_too_many_digits() {
        let mut buf = [0u8; 16];
        assert_eq!(build_mnt_path(10000, &mut buf), None);
    }
}
