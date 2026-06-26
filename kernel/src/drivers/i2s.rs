//! Audio I2S — Duo S sound output via the I2S peripheral.
//!
//! Duo S exposes a DesignWare I2S controller at 0x0309_0000. This driver
//! implements probe/init and a 16-bit stereo PCM stream API suitable for
//! simple beep/tone generation. Full ALSA-style ring buffers are out of
//! scope; the driver exposes a blocking `write_samples` call.
use crate::arch::mmio::Mmio;
use onyx_core::errno::{Errno, KResult};

const I2S_BASE: usize = 0x0309_0000;

const R_IER: u32 = 0x00;
const _R_IRER: u32 = 0x04;
const R_ITER: u32 = 0x08;
const R_CER: u32 = 0x0C;
const R_CCR: u32 = 0x10;
const _R_RXFFR: u32 = 0x14;
const R_TXFFR: u32 = 0x18;
const _R_LRBR: u32 = 0x40;
const R_LTHR: u32 = 0x40;
const R_RTHR: u32 = 0x44;
const _R_ISR: u32 = 0x60;
const _R_IMR: u32 = 0x64;
const _R_ROR: u32 = 0x68;
const _R_TOR: u32 = 0x6C;
const _R_RFCR: u32 = 0x70;
const _R_TFCR: u32 = 0x74;
const _R_RFF: u32 = 0x78;
const R_TFF: u32 = 0x7C;

const IER_EN: u32 = 1 << 0;
const ITER_EN: u32 = 1 << 0;
const CER_EN: u32 = 1 << 0;
const TFF_EMPTY: u32 = 1 << 5;

static mut G_BASE: usize = I2S_BASE;
static mut G_RATE: u32 = 44100;

#[inline]
unsafe fn rd(off: u32) -> u32 {
    Mmio::<u32>::at(G_BASE + off as usize).read()
}

#[inline]
unsafe fn wr(off: u32, v: u32) {
    Mmio::<u32>::at(G_BASE + off as usize).write(v);
}

/// Initialise the I2S controller for 16-bit stereo PCM at `sample_rate`.
/// `mclk_div` selects the master-clock divider (e.g. 0 = /1, 1 = /2, ...).
pub unsafe fn init(base: usize, sample_rate: u32, mclk_div: u32) -> KResult<()> {
    if base == 0 || sample_rate == 0 {
        return Err(Errno::Inval);
    }
    G_BASE = base;
    G_RATE = sample_rate;
    wr(R_IER, IER_EN);
    wr(R_ITER, ITER_EN);
    // 16-bit data, 2 channels per frame.
    wr(R_CCR, (mclk_div & 0xFF) << 16 | 0x10);
    // Configure word length and slot.
    wr(R_TXFFR, 0x3); // FIFO reset on TX
    wr(R_CER, CER_EN);
    Ok(())
}

/// Write a buffer of interleaved 16-bit stereo samples to the I2S FIFO.
/// Blocks until the entire buffer has been drained.
pub fn write_samples(samples: &[i16]) -> KResult<()> {
    if samples.len() % 2 != 0 {
        return Err(Errno::Inval);
    }
    unsafe {
        let mut i = 0;
        while i < samples.len() {
            // Wait for FIFO space.
            let mut t = 1_000_000u32;
            while t > 0 && rd(R_TFF) & TFF_EMPTY != 0 {
                t -= 1;
            }
            if t == 0 {
                return Err(Errno::Io);
            }
            let left = samples[i] as u32;
            let right = samples[i + 1] as u32;
            wr(R_LTHR, left & 0xFFFF);
            wr(R_RTHR, right & 0xFFFF);
            i += 2;
        }
        Ok(())
    }
}

/// Mute the controller (disable TX) without resetting state.
pub fn mute() {
    unsafe { wr(R_ITER, 0); }
}

/// Unmute the controller (re-enable TX).
pub fn unmute() {
    unsafe { wr(R_ITER, ITER_EN); }
}

/// Current configured sample rate.
pub fn sample_rate() -> u32 {
    unsafe { G_RATE }
}
