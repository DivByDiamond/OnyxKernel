use super::cmd::*;
use super::regs::*;
use super::{
    clear_interrupts, reset, set_sdma_addr, wait_idle, wait_state, wait_transfer_complete,
};

/// # Safety
/// Driver must be initialized (`crate::drivers::sdhci::init`); `lba` must be a valid sector.
pub unsafe fn read(lba: u64, buf: &mut [u8; 512]) -> bool {
    // SAFETY: G_SDHCI was filled by single-threaded init with a probed MMIO
    // base; register offsets per regs.rs; buf is a 512-byte buffer handed to
    // the SDMA engine and polled to completion.
    unsafe {
        let p = &raw const crate::drivers::sdhci::G_SDHCI;
        if !(*p).initialized {
            return false;
        }
        let base = (*p).base;

        wait_idle(base);
        reg_w16(base, BLOCK_SIZE, SDHCI_SECTOR_SIZE as u16);
        reg_w16(base, BLOCK_COUNT, 1);
        set_sdma_addr(base, buf.as_ptr() as u64);
        reg_w16(base, TRANSFER_MODE, TM_READ);

        let resp = send_command(
            base,
            CMD_READ_SINGLE_BLOCK,
            lba as u32,
            CMD_RESP_48 | CMD_CRC_ENABLE | CMD_INDEX_ENABLE,
            true,
        );
        if resp == 0xFFFF_FFFF {
            return false;
        }

        if !wait_state(base, PS_BUFFER_READ_ENABLE) {
            reset(base, SW_RESET_DAT);
            return false;
        }

        if !wait_transfer_complete(base) {
            return false;
        }

        let ps = reg_r(base, PRESENT_STATE);
        if ps & PS_BUFFER_READ_ENABLE != 0 {
            for i in (0..512).step_by(4) {
                let word = reg_r(base, BUFFER_DATA);
                buf[i] = word as u8;
                buf[i + 1] = (word >> 8) as u8;
                buf[i + 2] = (word >> 16) as u8;
                buf[i + 3] = (word >> 24) as u8;
            }
        }

        true
    }
}

/// # Safety
/// Driver must be initialized; `lba` must be a valid sector.
pub unsafe fn write(lba: u64, buf: &[u8; 512]) -> bool {
    // SAFETY: G_SDHCI was filled by single-threaded init with a probed MMIO
    // base; register offsets per regs.rs; buf stays valid for the polled
    // SDMA transfer of one 512-byte sector.
    unsafe {
        let p = &raw const crate::drivers::sdhci::G_SDHCI;
        if !(*p).initialized {
            return false;
        }
        let base = (*p).base;

        wait_idle(base);
        reg_w16(base, BLOCK_SIZE, SDHCI_SECTOR_SIZE as u16);
        reg_w16(base, BLOCK_COUNT, 1);
        set_sdma_addr(base, buf.as_ptr() as u64);
        clear_interrupts(base);
        reg_w16(base, TRANSFER_MODE, 0);

        let resp = send_command(
            base,
            CMD_WRITE_SINGLE_BLOCK,
            lba as u32,
            CMD_RESP_48 | CMD_CRC_ENABLE | CMD_INDEX_ENABLE,
            true,
        );
        if resp == 0xFFFF_FFFF {
            return false;
        }

        if !wait_state(base, PS_BUFFER_WRITE_ENABLE) {
            reset(base, SW_RESET_DAT);
            return false;
        }

        let ps = reg_r(base, PRESENT_STATE);
        if ps & PS_BUFFER_WRITE_ENABLE != 0 {
            for i in (0..512).step_by(4) {
                let word = (buf[i] as u32)
                    | ((buf[i + 1] as u32) << 8)
                    | ((buf[i + 2] as u32) << 16)
                    | ((buf[i + 3] as u32) << 24);
                reg_w(base, BUFFER_DATA, word);
            }
        }

        wait_transfer_complete(base)
    }
}

/// # Safety
/// Caller contract: `buf` must be writable for `n_sectors * 512` bytes; driver must be initialized.
pub unsafe fn read_multi(lba: u64, n_sectors: u32, buf: *mut u8) -> bool {
    // SAFETY: per-iteration offset i*512 is within the caller's buffer, which
    // must hold n_sectors*512 bytes (caller contract); copy is non-overlapping.
    unsafe {
        let mut sector_buf = [0u8; SDHCI_SECTOR_SIZE];
        for i in 0u32..n_sectors {
            if !read(lba + i as u64, &mut sector_buf) {
                return false;
            }
            // SAFETY: dest offset i*512 is bounded by the n_sectors*512 caller contract.
            core::ptr::copy_nonoverlapping(
                sector_buf.as_ptr(),
                buf.add((i as usize) * SDHCI_SECTOR_SIZE),
                SDHCI_SECTOR_SIZE,
            );
        }
        true
    }
}

/// # Safety
/// Caller contract: `buf` must be readable for `n_sectors * 512` bytes; driver must be initialized.
pub unsafe fn write_multi(lba: u64, n_sectors: u32, buf: *const u8) -> bool {
    // SAFETY: per-iteration offset i*512 is within the caller's buffer, which
    // must hold n_sectors*512 bytes (caller contract); copy is non-overlapping.
    unsafe {
        let mut sector_buf = [0u8; SDHCI_SECTOR_SIZE];
        for i in 0u32..n_sectors {
            // SAFETY: source offset i*512 is bounded by the n_sectors*512 caller contract.
            core::ptr::copy_nonoverlapping(
                buf.add((i as usize) * SDHCI_SECTOR_SIZE),
                sector_buf.as_mut_ptr(),
                SDHCI_SECTOR_SIZE,
            );
            if !write(lba + i as u64, &sector_buf) {
                return false;
            }
        }
        true
    }
}
