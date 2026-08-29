use crate::arch::regs::ONYXFS_LBA;
use crate::fs::vfs;
use crate::mm::heap;
use onyx_core::errno::KResult;
use onyx_core::fmt::Arg;

/// # Safety
///
/// Boot-time VFS bring-up: mounts root/procfs/ipcfs/devfs; must run
/// single-threaded on the boot hart after the block drivers are probed.
pub(crate) unsafe fn setup(ndevs: usize) {
    // SAFETY: one-shot boot call; `ndevs` comes from probe_devices and mount candidates are validated by the superblock check.
    unsafe {
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
}

/// # Safety
///
/// Reads /font/default.psf into a heap buffer and hands it to font::init.
/// The buffer is intentionally NOT freed: font::init retains raw pointers
/// into it (font/psf2.rs stores `glyphs`/`unicode` into G_FONT), so the
/// blob must outlive the kernel. Leak is bounded by the font file size.
pub(crate) unsafe fn load_font() {
    // SAFETY: from_raw_parts covers exactly the kmalloc'd `size` bytes; the buffer is leaked on purpose (font::init retains raw pointers into it) so the slice stays valid for the program's lifetime.
    unsafe {
        (|| -> KResult<()> {
            let token = vfs::open(b"/font/default.psf", vfs::PERM_READ)?;
            let mut size = 0u32;
            vfs::stat(token, &mut size).ok();
            if size > 0 {
                let buf = heap::kmalloc(size as usize)?;
                vfs::read(token, buf, size).ok();
                vfs::close(token).ok();
                crate::font::init(core::slice::from_raw_parts(buf, size as usize)).ok();
                // Leak `buf` on purpose: font::init kept raw pointers into
                // the PSF blob (G_FONT.glyphs/unicode). Freeing it would
                // leave every glyph read dangling (UAF on the console path).
                crate::kinf!("font", "loaded /font/default.psf");
            } else {
                vfs::close(token).ok();
            }
            Ok(())
        })()
        .unwrap_or_else(|_| crate::kwrn!("font", "no /font/default.psf, using blank font"));
    }
}
