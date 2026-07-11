//! PWM — pulse-width modulation for backlight and fans.
//!
//! SiFive PWM (QEMU virt) at 0x1002_0000 has 4 channels. Each channel
//! has a 16-bit period and a 16-bit duty cycle. The driver exposes
//! enable/disable, set_period, set_duty (in ticks), and convenience
//! helpers `set_duty_percent` and `set_freq_hz`.
use crate::arch::mmio::Mmio;
use onyx_core::errno::{Errno, KResult};

pub const PWM_BASE: usize = 0x1002_0000;
pub const N_CHANNELS: usize = 4;
const CLK_HZ: u32 = 50_000_000;

const R_CFG: u32 = 0x00;
const R_COUNT: u32 = 0x08;
const R_S: u32 = 0x0C;

#[derive(Clone, Copy)]
struct Channel {
    enabled: bool,
    period: u16,
    duty: u16,
}

static mut G_BASE: usize = PWM_BASE;
static mut G_CHANNELS: [Channel; N_CHANNELS] = [Channel {
    enabled: false,
    period: 0,
    duty: 0,
}; N_CHANNELS];

#[inline]
unsafe fn reg(chan: usize, off: u32) -> usize {
    G_BASE + 0x10 + chan * 0x10 + off as usize
}

#[inline]
unsafe fn _rd(chan: usize, off: u32) -> u32 {
    Mmio::<u32>::at(reg(chan, off)).read()
}

#[inline]
unsafe fn wr(chan: usize, off: u32, v: u32) {
    Mmio::<u32>::at(reg(chan, off)).write(v);
}

pub unsafe fn init(base: usize) {
    G_BASE = base;
    // Scale: bits 0..3 = scale value; 0 = no scaling.
    Mmio::<u32>::at(G_BASE + R_CFG as usize).write(0);
}

fn check_chan(chan: usize) -> KResult<()> {
    if chan >= N_CHANNELS {
        return Err(Errno::Inval);
    }
    Ok(())
}

/// Set the period (in clock ticks) of a channel.
pub fn set_period(chan: usize, period: u16) -> KResult<()> {
    check_chan(chan)?;
    unsafe {
        G_CHANNELS[chan].period = period;
        wr(chan, R_COUNT, period as u32);
    }
    Ok(())
}

/// Set the duty cycle (in clock ticks, must be <= period).
pub fn set_duty(chan: usize, duty: u16) -> KResult<()> {
    check_chan(chan)?;
    unsafe {
        let p = G_CHANNELS[chan].period;
        if duty > p {
            return Err(Errno::Range);
        }
        G_CHANNELS[chan].duty = duty;
        wr(chan, R_S, duty as u32);
    }
    Ok(())
}

/// Set duty as a percentage (0..=100) of the current period.
pub fn set_duty_percent(chan: usize, pct: u8) -> KResult<()> {
    if pct > 100 {
        return Err(Errno::Range);
    }
    unsafe {
        let p = G_CHANNELS[chan].period as u32;
        let d = (p * pct as u32) / 100;
        set_duty(chan, d as u16)
    }
}

/// Configure a channel for a target frequency in Hz. Computes the
/// period from the underlying clock and sets a 50% duty cycle.
pub fn set_freq_hz(chan: usize, hz: u32) -> KResult<()> {
    if hz == 0 || hz > CLK_HZ / 16 {
        return Err(Errno::Range);
    }
    let period = (CLK_HZ / hz) as u16;
    set_period(chan, period)?;
    set_duty(chan, period / 2)?;
    Ok(())
}

/// Enable a channel's output.
pub fn enable(chan: usize) -> KResult<()> {
    check_chan(chan)?;
    unsafe {
        G_CHANNELS[chan].enabled = true;
        let cur = Mmio::<u32>::at(G_BASE + R_CFG as usize).read();
        Mmio::<u32>::at(G_BASE + R_CFG as usize).write(cur | (1 << (chan + 4)));
    }
    Ok(())
}

/// Disable a channel's output.
pub fn disable(chan: usize) -> KResult<()> {
    check_chan(chan)?;
    unsafe {
        G_CHANNELS[chan].enabled = false;
        let cur = Mmio::<u32>::at(G_BASE + R_CFG as usize).read();
        Mmio::<u32>::at(G_BASE + R_CFG as usize).write(cur & !(1 << (chan + 4)));
    }
    Ok(())
}

/// Is the channel currently enabled?
pub fn is_enabled(chan: usize) -> bool {
    if chan >= N_CHANNELS {
        return false;
    }
    unsafe { G_CHANNELS[chan].enabled }
}
