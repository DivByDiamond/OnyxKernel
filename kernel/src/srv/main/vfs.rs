use crate::arch::regs::ONYXFS_LBA;
use crate::fs::vfs;
use crate::mm::heap;
use onyx_core::errno::KResult;
use onyx_core::fmt::Arg;

pub(crate) unsafe fn setup(ndevs: usize) {
    vfs::init();
    if ndevs > 0 {
        // OC2R: the OnyxFS disk is not necessarily dev 0 (vda=bootfs,
        // vdb=rootfs, vdc=first HDD). Scan all probed virtio-blk devices and
        // mount the first one carrying a valid filesystem superblock.
        // mount_root tries OnyxFS then FAT32 and fails cleanly on others.
        let mut mounted = false;
        for dev in 0..ndevs {
            // A standalone OnyxFS hard drive (OC2R) has its superblock at LBA 0;
            // QEMU embeds the image at LBA 10240 of the boot disk. Try LBA 0
            // first — reading LBA 10240 on a small standalone drive is out of
            // range and the virtio device never completes the request (hang).
            if vfs::mount_root(dev, 0).is_ok() || vfs::mount_root(dev, ONYXFS_LBA).is_ok() {
                crate::kinf!("vfs", "root mounted on dev %d", Arg::from(dev as u64));
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
}

pub(crate) unsafe fn load_font() {
    (|| -> KResult<()> {
        let token = vfs::open(b"/font/default.psf", vfs::PERM_READ)?;
        let mut size = 0u32;
        vfs::stat(token, &mut size).ok();
        if size > 0 {
            let buf = heap::kmalloc(size as usize)?;
            vfs::read(token, buf, size).ok();
            vfs::close(token).ok();
            crate::font::init(core::slice::from_raw_parts(buf, size as usize)).ok();
            heap::kfree(buf);
            crate::kinf!("font", "loaded /font/default.psf");
        } else {
            vfs::close(token).ok();
        }
        Ok(())
    })()
    .unwrap_or_else(|_| crate::kwrn!("font", "no /font/default.psf, using blank font"));
}
