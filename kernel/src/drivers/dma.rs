//! SiFive DMA engine — channel allocation + descriptor submission.
//!
//! SiFive FU540 DMA has 4 channels; each supports memory-to-memory
//! and memory-to-peripheral transfers with up to 2 descriptors per
//! chain. The driver exposes channel allocation, simple mem-to-mem
//! copy, and a low-level `submit` for chained transfers.
use crate::arch::mmio::Mmio;
use onyx_core::errno::{Errno, KResult};

pub const DMA_BASE: usize = 0x3000_000;
pub const N_CHANNELS: usize = 4;

const R_NEXT_DEST: u32 = 0x00;
const R_NEXT_CONFIG: u32 = 0x04;
const R_NEXT_BYTES: u32 = 0x08;
const R_NEXT_SRC: u32 = 0x0C;
const _R_EXEC_DEST: u32 = 0x10;
const _R_EXEC_CONFIG: u32 = 0x14;
const _R_EXEC_BYTES: u32 = 0x18;
const _R_EXEC_SRC: u32 = 0x1C;

const CFG_DONE_IE: u32 = 1 << 1;
const _CFG_ERR_IE: u32 = 1 << 2;
const _CFG_REPEAT: u32 = 1 << 4;
const CFG_MEM_TO_MEM: u32 = 1 << 13;
const CFG_RUN: u32 = 1 << 15;

#[derive(Clone, Copy)]
struct Channel {
    in_use: bool,
}

static mut G_BASE: usize = DMA_BASE;
static mut G_CHANNELS: [Channel; N_CHANNELS] = [Channel { in_use: false }; N_CHANNELS];

#[inline]
unsafe fn reg(chan: usize, off: u32) -> usize {
    G_BASE + chan * 0x20 + off as usize
}

#[inline]
unsafe fn rd(chan: usize, off: u32) -> u32 {
    Mmio::<u32>::at(reg(chan, off)).read()
}

#[inline]
unsafe fn wr(chan: usize, off: u32, v: u32) {
    Mmio::<u32>::at(reg(chan, off)).write(v);
}

pub unsafe fn init(base: usize) {
    G_BASE = base;
    for c in 0..N_CHANNELS {
        G_CHANNELS[c] = Channel { in_use: false };
    }
}

/// Allocate a free DMA channel. Returns the channel index.
pub fn alloc() -> KResult<usize> {
    unsafe {
        for c in 0..N_CHANNELS {
            if !G_CHANNELS[c].in_use {
                G_CHANNELS[c].in_use = true;
                return Ok(c);
            }
        }
        Err(Errno::Busy)
    }
}

/// Release a previously-allocated channel.
pub fn free(chan: usize) -> KResult<()> {
    if chan >= N_CHANNELS {
        return Err(Errno::Inval);
    }
    unsafe {
        if !G_CHANNELS[chan].in_use {
            return Err(Errno::Inval);
        }
        wr(chan, R_NEXT_CONFIG, 0);
        G_CHANNELS[chan].in_use = false;
    }
    Ok(())
}

/// Synchronous memory-to-memory copy. `dst` and `src` are physical
/// addresses; `len` is the number of bytes (must be multiple of 4).
pub fn copy(dst: usize, src: usize, len: usize) -> KResult<()> {
    if len == 0 || len % 4 != 0 {
        return Err(Errno::Inval);
    }
    let chan = alloc()?;
    unsafe {
        wr(chan, R_NEXT_SRC, src as u32);
        wr(chan, R_NEXT_DEST, dst as u32);
        wr(chan, R_NEXT_BYTES, len as u32);
        wr(chan, R_NEXT_CONFIG, CFG_MEM_TO_MEM | CFG_DONE_IE | CFG_RUN);
        // Wait for RUN bit to clear.
        let mut t = 10_000_000u32;
        while t > 0 && rd(chan, R_NEXT_CONFIG) & CFG_RUN != 0 {
            t -= 1;
        }
        if t == 0 {
            free(chan)?;
            return Err(Errno::Io);
        }
    }
    free(chan)?;
    Ok(())
}

/// Submit a low-level descriptor for a chained transfer. The caller
/// is responsible for setting up the next-link field if chaining.
pub fn submit(chan: usize, src: usize, dst: usize, len: usize) -> KResult<()> {
    if chan >= N_CHANNELS || len == 0 || len % 4 != 0 {
        return Err(Errno::Inval);
    }
    unsafe {
        if !G_CHANNELS[chan].in_use {
            return Err(Errno::Inval);
        }
        wr(chan, R_NEXT_SRC, src as u32);
        wr(chan, R_NEXT_DEST, dst as u32);
        wr(chan, R_NEXT_BYTES, len as u32);
        wr(chan, R_NEXT_CONFIG, CFG_MEM_TO_MEM | CFG_RUN);
    }
    Ok(())
}

/// Poll the channel for completion. Returns `true` if the channel has
/// finished its current transfer.
pub fn is_done(chan: usize) -> bool {
    if chan >= N_CHANNELS {
        return true;
    }
    unsafe { rd(chan, R_NEXT_CONFIG) & CFG_RUN == 0 }
}
