use crate::arch::regs::ONYXFS_LBA;
use crate::fs::vfs;
use crate::mm::heap;
use onyx_core::errno::KResult;
use onyx_core::fmt::Arg;

pub(crate) unsafe fn setup(ndevs: usize) {
    vfs::init();
    if ndevs > 0 {
        match vfs::mount_root(0, ONYXFS_LBA) {
            Ok(()) => crate::kinf!("vfs", "root mounted"),
            Err(e) => crate::kerr!("vfs", "mount failed: %s", Arg::from(e.as_str())),
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
