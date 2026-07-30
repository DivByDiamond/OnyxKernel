//! GPIO pin-level operations: direction, drive, read, edge IRQ.
use super::{
    G_PINS, N_PINS, R_FALL_IE, R_FALL_IP, R_INPUT_EN, R_INPUT_VAL, R_OUT_XOR, R_OUTPUT_EN,
    R_OUTPUT_VAL, R_RISE_IE, R_RISE_IP, rd, wr,
};
use onyx_core::errno::{Errno, KResult};

pub type PinHandler = fn(usize);

#[derive(Clone, Copy)]
pub struct PinSlot {
    pub handler: Option<PinHandler>,
}

#[inline]
fn mask(pin: usize) -> KResult<u32> {
    if pin >= N_PINS {
        return Err(Errno::Inval);
    }
    Ok(1u32 << pin)
}

pub fn set_output(pin: usize) -> KResult<()> {
    let m = mask(pin)?;
    unsafe {
        wr(R_OUTPUT_EN, rd(R_OUTPUT_EN) | m);
        wr(R_INPUT_EN, rd(R_INPUT_EN) & !m);
    }
    Ok(())
}

pub fn set_input(pin: usize) -> KResult<()> {
    let m = mask(pin)?;
    unsafe {
        wr(R_INPUT_EN, rd(R_INPUT_EN) | m);
        wr(R_OUTPUT_EN, rd(R_OUTPUT_EN) & !m);
    }
    Ok(())
}

pub fn write(pin: usize, value: u8) -> KResult<()> {
    let m = mask(pin)?;
    unsafe {
        let cur = rd(R_OUTPUT_VAL);
        wr(R_OUTPUT_VAL, if value != 0 { cur | m } else { cur & !m });
    }
    Ok(())
}

pub fn read(pin: usize) -> KResult<u8> {
    let m = mask(pin)?;
    Ok(unsafe { if rd(R_INPUT_VAL) & m != 0 { 1 } else { 0 } })
}

pub fn toggle(pin: usize) -> KResult<()> {
    let m = mask(pin)?;
    unsafe {
        wr(R_OUTPUT_VAL, rd(R_OUTPUT_VAL) ^ m);
    }
    Ok(())
}

pub fn set_invert(pin: usize, on: bool) -> KResult<()> {
    let m = mask(pin)?;
    unsafe {
        let cur = rd(R_OUT_XOR);
        wr(R_OUT_XOR, if on { cur | m } else { cur & !m });
    }
    Ok(())
}

pub fn on_edge(pin: usize, rising: bool, h: PinHandler) -> KResult<()> {
    let m = mask(pin)?;
    unsafe {
        let p = &raw mut G_PINS;
        (*p)[pin].handler = Some(h);
        if rising {
            wr(R_RISE_IE, rd(R_RISE_IE) | m);
        } else {
            wr(R_FALL_IE, rd(R_FALL_IE) | m);
        }
    }
    Ok(())
}

/// Dispatch pending GPIO interrupts. Called from the PLIC handler.
pub unsafe fn dispatch(rising: bool) {
    let pending = if rising { rd(R_RISE_IP) } else { rd(R_FALL_IP) };
    if pending == 0 {
        return;
    }
    if rising {
        wr(R_RISE_IP, pending);
    } else {
        wr(R_FALL_IP, pending);
    }
    let p = &raw const G_PINS;
    for pin in 0..N_PINS {
        if pending & (1 << pin) == 0 {
            continue;
        }
        if let Some(h) = (*p)[pin].handler {
            h(pin);
        }
    }
}
