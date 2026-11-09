//! SiFive DMA engine — channel allocation + descriptor submission.
//!
//! SiFive FU540 DMA has 4 channels; each supports memory-to-memory
//! and memory-to-peripheral transfers with up to 2 descriptors per
//! chain. The driver exposes channel allocation, simple mem-to-mem
//! copy, and a low-level `submit` for chained transfers.
use crate::arch::mmio::Mmio;
use onyx_core::errno::{Errno, KResult};

pub const DMA_BASE: usize = 0x0300_0000;
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
/// # Safety
///
/// Caller contract: `chan` must be < N_CHANNELS and `off` a DMA register
/// offset; G_BASE must be initialised (DMA_BASE default or from `init`).
unsafe fn reg(chan: usize, off: u32) -> usize {
    // SAFETY: G_BASE is the fixed SoC/FDT DMA base; chan*0x20 + off are datasheet channel/stride constants, keeping the address in the DMA MMIO window.
    unsafe { G_BASE + chan * 0x20 + off as usize }
}

#[inline]
/// # Safety
///
/// Caller contract: same as `reg` — `chan` < N_CHANNELS, `off` a valid
/// DMA register offset, G_BASE initialised.
unsafe fn rd(chan: usize, off: u32) -> u32 {
    // SAFETY: reg() computes an address inside the DMA MMIO window from the validated G_BASE; Mmio::read is the designated MMIO accessor.
    unsafe { Mmio::<u32>::at(reg(chan, off)).read() }
}

#[inline]
/// # Safety
///
/// Caller contract: same as `reg` — `chan` < N_CHANNELS, `off` a valid
/// DMA register offset, G_BASE initialised.
unsafe fn wr(chan: usize, off: u32, v: u32) {
    // SAFETY: reg() computes an address inside the DMA MMIO window from the validated G_BASE; Mmio::write is the designated MMIO accessor.
    unsafe {
        Mmio::<u32>::at(reg(chan, off)).write(v);
    }
}

/// # Safety
///
/// Caller contract: `base` must be the validated DMA MMIO base (FDT node
/// or DMA_BASE constant) and `init` must run once, on a single hart,
/// before any channel use.
pub unsafe fn init(base: usize) {
    // SAFETY: single-threaded init before harts allocate channels; G_BASE/G_CHANNELS only written here.
    unsafe {
        G_BASE = base;
        G_CHANNELS = [Channel { in_use: false }; N_CHANNELS];
    }
}

/// Allocate a free DMA channel. Returns the channel index.
pub fn alloc() -> KResult<usize> {
    // SAFETY: exclusive mutable access to G_CHANNELS is handed out only here, returning before further accesses; no reentrancy (SIE=0).
    unsafe {
        for (c, ch) in G_CHANNELS.iter_mut().enumerate() {
            if !ch.in_use {
                ch.in_use = true;
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
    // SAFETY: chan bounds-checked against N_CHANNELS; G_CHANNELS access and wr() MMIO run with no concurrent channel use (SIE=0).
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
    if len == 0 || !len.is_multiple_of(4) {
        return Err(Errno::Inval);
    }
    let chan = alloc()?;
    // SAFETY: chan comes from alloc() (< N_CHANNELS); wr/rd hit DMA registers via the validated G_BASE; src/dst/len were validated by the caller (multiple of 4, physical addresses).
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
    if chan >= N_CHANNELS || len == 0 || !len.is_multiple_of(4) {
        return Err(Errno::Inval);
    }
    // SAFETY: chan bounds-checked against N_CHANNELS; MMIO writes go through the validated G_BASE; src/dst/len validated above.
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
    // SAFETY: chan bounds-checked against N_CHANNELS; rd() reads a DMA register via the validated G_BASE.
    unsafe { rd(chan, R_NEXT_CONFIG) & CFG_RUN == 0 }
}
