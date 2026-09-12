//! Boot-time filesystem bring-up for root, pseudo mounts, and disks.

mod auto;
mod font;
mod probe;

pub(crate) use font::load_font;

use crate::arch::regs::ONYXFS_LBA;
use crate::fs::vfs;
use onyx_core::fmt::Arg;

/// # Safety
///
/// Boot-time VFS bring-up: mounts root/procfs/ipcfs/devfs and auto-mounts
/// secondary disks; must run single-threaded on the boot hart after the
/// block drivers are probed.
pub(crate) unsafe fn setup(ndevs: usize) {
    // SAFETY: one-shot boot call; `ndevs` comes from probe_devices, mount
    // candidates are validated by filesystem superblock checks, and the
    // mount table is written before user processes are scheduled.
    unsafe {
        vfs::init();
        let mut root_dev: Option<usize> = None;
        if ndevs > 0 {
            // OC2R: the OnyxFS disk is not necessarily dev 0 (vda=bootfs,
            // vdb=rootfs, vdc=first HDD). Scan all probed virtio-blk devices
            // and mount the first one carrying a valid filesystem.
            let mut mounted = false;
            for dev in 0..ndevs {
                // A standalone OnyxFS hard drive (OC2R) has its superblock at
                // LBA 0; QEMU embeds the image at LBA 10240 of the boot disk.
                // Try LBA 0 first — reading LBA 10240 on a small standalone
                // drive is out of range and the virtio device never completes.
                if vfs::mount_root(dev, 0).is_ok() || vfs::mount_root(dev, ONYXFS_LBA).is_ok() {
                    crate::kinf!("vfs", "root mounted on dev %d", Arg::from(dev as u64));
                    root_dev = Some(dev);
                    mounted = true;
                    break;
                }
                crate::kwrn!(
                    "vfs",
                    "dev %d: no bootable filesystem",
                    Arg::from(dev as u64)
                );
            }
            if !mounted {
                crate::kerr!(
                    "vfs",
                    "mount failed on all %d device(s)",
                    Arg::from(ndevs as u64)
                );
            }
        }

        vfs::mount_procfs();
        crate::kinf!("vfs", "procfs mounted at /proc");
        vfs::mount_ipcfs();
        crate::kinf!("vfs", "ipcfs mounted at /ipc");
        vfs::mount_devfs();
        crate::kinf!("vfs", "devfs mounted at /dev");

        auto::mount_secondary_disks(ndevs, root_dev);
    }
}
