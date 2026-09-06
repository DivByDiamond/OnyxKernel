//! ramfb — QEMU's simple RAM framebuffer via fw_cfg.
//!
//! QEMU's `-device ramfb` exposes a fw_cfg file `etc/ramfb` (selector
//! allocated dynamically, found via the FW_CFG_FILE_DIR directory at
//! selector 0x0019). Writing a `RamfbCfg` struct to that file configures
//! the host to display the guest RAM region at `addr` as a framebuffer.
//! No virtio queue is involved — the host simply scans the RAM region
//! for display, which makes it ideal for headless QEMU visual tests
//! (no vgabios, no virtio queue).

use crate::arch::mmio::Mmio;
use crate::mm::pmm;
use onyx_core::errno::{Errno, KResult};
use onyx_core::fmt::Arg;

const FW_CFG_BASE: usize = 0x10100000;
const FW_CFG_DATA: usize = FW_CFG_BASE;
const FW_CFG_SELECTOR: usize = FW_CFG_BASE + 8;
#[allow(dead_code)]
const FW_CFG_DMA: usize = FW_CFG_BASE + 16;

const FW_CFG_FILE_DIR: u16 = 0x0019;

#[allow(dead_code)]
#[repr(C, packed)]
struct FwCfgFile {
    size: u32,
    select: u16,
    reserved: u16,
    name: [u8; 56],
}

#[repr(C, packed)]
struct RamfbCfg {
    addr: u64,
    fourcc: u32,
    flags: u32,
    width: u32,
    height: u32,
    stride: u32,
}

/// Find the selector for `etc/ramfb` via the fw_cfg directory.
unsafe fn find_ramfb_selector() -> Option<u16> {
    unsafe {
        // Select the file directory (fw_cfg selector is big-endian on the wire).
        Mmio::<u16>::at(FW_CFG_SELECTOR).write(FW_CFG_FILE_DIR.to_be());
        // First 4 bytes: number of files (big-endian u32).
        let mut count: u32 = 0;
        for _ in 0..4 {
            let b = Mmio::<u8>::at(FW_CFG_DATA).read();
            count = (count << 8) | b as u32;
        }
        crate::kinf!("ramfb", "fw_cfg count=%d", Arg::from(count));
        if count == 0 || count > 64 {
            crate::kwrn!("ramfb", "bad count %d", Arg::from(count));
            return None;
        }
        // Each file entry is 64 bytes: size(4) select(2) reserved(2) name(56).
        for _ in 0..count as usize {
            let mut size: u32 = 0;
            for _ in 0..4 {
                let b = Mmio::<u8>::at(FW_CFG_DATA).read();
                size = (size << 8) | b as u32;
            }
            let mut sel: u16 = 0;
            for _ in 0..2 {
                let b = Mmio::<u8>::at(FW_CFG_DATA).read();
                sel = (sel << 8) | b as u16;
            }
            // Skip reserved (2 bytes).
            for _ in 0..2 {
                let _ = Mmio::<u8>::at(FW_CFG_DATA).read();
            }
            let mut name = [0u8; 56];
            #[allow(clippy::needless_range_loop)]
            for j in 0..56 {
                name[j] = Mmio::<u8>::at(FW_CFG_DATA).read();
            }
            // Compare name with "etc/ramfb".
            if size as usize == core::mem::size_of::<RamfbCfg>() {
                let target = b"etc/ramfb";
                let mut ok = true;
                for (j, &c) in target.iter().enumerate() {
                    if name[j] != c {
                        ok = false;
                        break;
                    }
                }
                if ok && name[target.len()] == 0 {
                    return Some(sel);
                }
            }
        }
        None
    }
}

/// Initialize a 1280x720 ramfb framebuffer and configure QEMU to display it.
/// Returns the framebuffer physical address on success.
pub unsafe fn init(width: u32, height: u32) -> KResult<usize> {
    unsafe {
        let sel = find_ramfb_selector().ok_or(Errno::NoEnt)?;
        let fb_pages = (width as usize * height as usize * 4).div_ceil(4096);
        let fb_pa = pmm::alloc_n(fb_pages)? as usize;
        let cfg = RamfbCfg {
            addr: (fb_pa as u64).to_be(),
            fourcc: 0x34325258u32.to_be(),
            flags: 0u32.to_be(),
            width: width.to_be(),
            height: height.to_be(),
            stride: (width * 4).to_be(),
        };
        // Select the ramfb file.
        Mmio::<u16>::at(FW_CFG_SELECTOR).write(sel.to_be());
        // Write the config struct via the data register (big-endian bytes).
        let bytes = core::slice::from_raw_parts(
            &cfg as *const _ as *const u8,
            core::mem::size_of::<RamfbCfg>(),
        );
        for &b in bytes {
            Mmio::<u8>::at(FW_CFG_DATA).write(b);
        }
        crate::kinf!(
            "ramfb",
            "configured at %p %dx%d",
            Arg::from(fb_pa),
            Arg::from(width),
            Arg::from(height)
        );
        Ok(fb_pa)
    }
}
