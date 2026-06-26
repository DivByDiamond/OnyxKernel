//! Canaan SG2000/SG2042 pinctrl — pad multiplexing.
//!
//! The SG series has a 64-pin pad matrix where each pad can be routed
//! to one of up to 8 functions. The driver exposes a tiny API to set
//! the mux of a single pad and to read back its current configuration.
use crate::arch::mmio::Mmio;
use onyx_core::errno::{Errno, KResult};

const PINCTRL_BASE: usize = 0x0302_0000;
const PADS_PER_REG: u32 = 8;
const FUNC_BITS: u32 = 3;
const FUNC_MASK: u32 = (1 << FUNC_BITS) - 1;

static mut G_BASE: usize = PINCTRL_BASE;

#[inline]
unsafe fn reg_for_pad(pad: u32) -> u32 {
    (pad / PADS_PER_REG) * 4
}

#[inline]
unsafe fn shift_for_pad(pad: u32) -> u32 {
    (pad % PADS_PER_REG) * FUNC_BITS
}

/// Initialise the pinctrl base address.
pub unsafe fn init(base: usize) {
    G_BASE = base;
}

/// Set the mux function of a single pad (0..N_PADS). `func` is a 3-bit
/// function selector. Most pads have function 0 = GPIO, function 1..7
/// depend on the specific pad (see the SG2000 datasheet).
pub fn set_mux(pad: u32, func: u32) -> KResult<()> {
    if pad >= 256 || func > FUNC_MASK {
        return Err(Errno::Inval);
    }
    unsafe {
        let off = reg_for_pad(pad);
        let sh = shift_for_pad(pad);
        let cur = Mmio::<u32>::at(G_BASE + off as usize).read();
        let next = (cur & !(FUNC_MASK << sh)) | ((func & FUNC_MASK) << sh);
        Mmio::<u32>::at(G_BASE + off as usize).write(next);
    }
    Ok(())
}

/// Read the current mux function of a pad.
pub fn get_mux(pad: u32) -> KResult<u32> {
    if pad >= 256 {
        return Err(Errno::Inval);
    }
    unsafe {
        let off = reg_for_pad(pad);
        let sh = shift_for_pad(pad);
        let v = Mmio::<u32>::at(G_BASE + off as usize).read();
        Ok((v >> sh) & FUNC_MASK)
    }
}

/// Configure a pad as GPIO (function 0).
pub fn set_gpio(pad: u32) -> KResult<()> {
    set_mux(pad, 0)
}

/// Apply a batch of (pad, func) pairs. Convenient for board setup.
pub fn apply_batch(pairs: &[(u32, u32)]) -> KResult<()> {
    for &(pad, func) in pairs {
        set_mux(pad, func)?;
    }
    Ok(())
}
