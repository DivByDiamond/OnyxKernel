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
const FW_CFG_DMA: usize = FW_CFG_BASE + 16;

const FW_CFG_FILE_DIR: u16 = 0x0019;

/// DMA control bits (QEMU fw_cfg spec): bit3 selects a file (upper 16 bits
/// carry its selector), bit4 requests a write. Bit1 (read) must stay clear.
const FW_CFG_DMA_CTL_SELECT: u32 = 0x08;
const FW_CFG_DMA_CTL_WRITE: u32 = 0x10;

// FwCfgFile layout kept as comment for spec reference (QEMU fw_cfg file dir entry):
// { size:u32, select:u16, reserved:u16, name:[u8;56] } — parsed manually via
// Mmio reads in find_ramfb_selector to avoid packing/endianness pitfalls.

/// Layout QEMU's fw_cfg DMA engine expects at the address written to
/// `FW_CFG_DMA`: control, then length, then the target buffer address —
/// all big-endian, packed with no gaps (spec: "the field at the lowest
/// address is the control field").
#[repr(C, packed)]
struct FwCfgDmaAccess {
    control: u32,
    length: u32,
    address: u64,
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
            for slot in &mut name {
                *slot = Mmio::<u8>::at(FW_CFG_DATA).read();
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
        // Since QEMU 2.9, plain writes to the fw_cfg data register are
        // no-ops — the guest must use the DMA interface to actually push
        // bytes into a write-callback file like etc/ramfb (spec: "writes
        // are reinstated, but only through the DMA interface"). Byte-by-
        // byte writes to FW_CFG_DATA silently do nothing, which is why
        // this used to report "configured" while QEMU never switched the
        // display away from the placeholder surface.
        let dma_pa = pmm::alloc_zero()? as usize;
        let dma = dma_pa as *mut FwCfgDmaAccess;
        core::ptr::write_volatile(
            dma,
            FwCfgDmaAccess {
                control: (((sel as u32) << 16) | FW_CFG_DMA_CTL_SELECT | FW_CFG_DMA_CTL_WRITE)
                    .to_be(),
                length: (core::mem::size_of::<RamfbCfg>() as u32).to_be(),
                address: (&cfg as *const RamfbCfg as u64).to_be(),
            },
        );
        // A single 64-bit write of the (big-endian) DMA-access-struct
        // address triggers the transfer.
        Mmio::<u64>::at(FW_CFG_DMA).write((dma_pa as u64).to_be());
        // Poll for completion: QEMU clears `control` to 0 on success and
        // sets bit0 (error) on failure; this transfer is synchronous on
        // current QEMU but the spec allows bits to linger briefly.
        let mut spins = 0u32;
        loop {
            let ctrl = u32::from_be(core::ptr::read_volatile(core::ptr::addr_of!(
                (*dma).control
            )));
            if ctrl == 0 {
                break;
            }
            if ctrl & 1 != 0 {
                crate::kwrn!("ramfb", "dma write error ctrl=0x%x", Arg::from(ctrl));
                return Err(Errno::Io);
            }
            spins += 1;
            if spins > 5_000_000 {
                crate::kwrn!("ramfb", "dma write timeout ctrl=0x%x", Arg::from(ctrl));
                return Err(Errno::Io);
            }
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
