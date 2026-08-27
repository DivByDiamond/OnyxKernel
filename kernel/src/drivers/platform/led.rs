//! LED driver — abstract indicator backed by GPIO pins.
//!
//! Provides a tiny abstraction so higher layers (init, network stack,
//! panic handler) can blink LEDs without knowing the underlying pin
//! wiring. Up to 8 logical LEDs are supported, each mapped to a
//! (gpio-pin, active-low?) pair set up via `bind()`.
use crate::drivers::gpio;
use onyx_core::errno::{Errno, KResult};

pub const MAX_LEDS: usize = 8;

/// Logical LED names. Extend as new indicators are needed.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Led {
    Power = 0,
    Activity = 1,
    Network = 2,
    Error = 3,
    User1 = 4,
    User2 = 5,
    User3 = 6,
    User4 = 7,
}

#[derive(Clone, Copy)]
struct Slot {
    pin: usize,
    active_low: bool,
    bound: bool,
}

static mut G_SLOTS: [Slot; MAX_LEDS] = [Slot {
    pin: 0,
    active_low: false,
    bound: false,
}; MAX_LEDS];

#[inline]
fn idx(led: Led) -> usize {
    led as usize
}

/// Bind a logical LED to a GPIO pin. `active_low` is true for LEDs that
/// light up when the pin is driven low (common on dev boards).
pub fn bind(led: Led, pin: usize, active_low: bool) -> KResult<()> {
    let i = idx(led);
    if i >= MAX_LEDS {
        return Err(Errno::Range);
    }
    gpio::set_output(pin)?;
    unsafe {
        G_SLOTS[i] = Slot {
            pin,
            active_low,
            bound: true,
        };
    }
    // Default: power LED on, everything else off.
    let initial_on = matches!(led, Led::Power);
    set(led, initial_on).ok();
    Ok(())
}

/// Drive the LED on (true) or off (false).
pub fn set(led: Led, on: bool) -> KResult<()> {
    let i = idx(led);
    let slot = unsafe { G_SLOTS[i] };
    if !slot.bound {
        return Err(Errno::NoEnt);
    }
    let raw = if slot.active_low { !on } else { on };
    gpio::write(slot.pin, if raw { 1 } else { 0 })?;
    Ok(())
}

/// Toggle the LED state.
pub fn toggle(led: Led) -> KResult<()> {
    let i = idx(led);
    let slot = unsafe { G_SLOTS[i] };
    if !slot.bound {
        return Err(Errno::NoEnt);
    }
    gpio::toggle(slot.pin)
}

/// Read back the current LED drive state (not the LED itself).
pub fn get(led: Led) -> KResult<bool> {
    let i = idx(led);
    let slot = unsafe { G_SLOTS[i] };
    if !slot.bound {
        return Err(Errno::NoEnt);
    }
    let raw = gpio::read(slot.pin)? != 0;
    Ok(if slot.active_low { !raw } else { raw })
}

/// Convenience: blink `led` `n` times with ~50ms spacing. Blocks the
/// caller — intended for panic / boot-error signalling.
pub fn blink(led: Led, n: u32) {
    for _ in 0..n {
        set(led, true).ok();
        delay(50_000);
        set(led, false).ok();
        delay(50_000);
    }
}

/// Pulse the activity LED briefly. Cheap to call from hot paths — the
/// driver keeps the LED on for ~5ms then turns it off.
pub fn pulse_activity() {
    if set(Led::Activity, true).is_ok() {
        delay(5_000);
        set(Led::Activity, false).ok();
    }
}

fn delay(loops: u32) {
    for _ in 0..loops {
        unsafe { core::arch::asm!("nop") }
    }
}
