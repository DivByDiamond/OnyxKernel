pub mod regs;
pub mod io;
pub mod cmd;
pub mod card;
pub mod block;

use self::regs::*;
use self::io::*;
use self::cmd::*;
use self::card::*;
use crate::drivers::plic;

pub use self::block::{read, write, read_multi, write_multi};
pub use self::regs::PLIC_PRIO_SDHCI;

#[derive(Clone, Copy)]
pub struct SdhciDev {
    pub base: usize,
    pub rca: u32,
    pub initialized: bool,
    pub irq: u32,
}

pub(crate) static mut G_SDHCI: SdhciDev = SdhciDev {
    base: 0,
    rca: 0,
    initialized: false,
    irq: PLIC_PRIO_SDHCI,
};

pub unsafe fn probe(base: usize) -> bool {
    let version = reg_r16(base, 0xFE);
    if version == 0 {
        return false;
    }
    let caps = reg_r(base, 0x40);
    if (caps >> 30) & 0x3 != 0 {
        return false;
    }
    reset(base, SW_RESET_ALL);
    let ps = reg_r(base, PRESENT_STATE);
    ps & PS_CARD_INSERTED != 0
}

pub unsafe fn init(base: usize, irq: u32) -> bool {
    reset(base, SW_RESET_ALL);
    reg_w(base, NORMAL_INT_STATUS_ENABLE, 0xFFFF);
    reg_w(base, ERROR_INT_STATUS_ENABLE, 0xFFFF);
    init_clock(base);
    set_power(base);
    if !wait_state(base, PS_CARD_STATE_STABLE) {
        return false;
    }
    reg_w(base, TIMEOUT_CONTROL, 0x0E);

    let rca = match card_init(base) {
        Some(r) => r,
        None => return false,
    };

    let p = &raw mut G_SDHCI;
    (*p).base = base;
    (*p).rca = rca;
    (*p).initialized = true;
    (*p).irq = irq;

    plic::set_priority(irq, 5);
    plic::enable(irq, 0);

    true
}

pub unsafe fn irq_handler() {
    let p = &raw const G_SDHCI;
    if !(*p).initialized {
        return;
    }
    let base = (*p).base;
    let istat = reg_r(base, NORMAL_INT_STATUS);
    reg_w(base, NORMAL_INT_STATUS, istat);
    let err = reg_r(base, ERROR_INT_STATUS);
    if err != 0 {
        reg_w(base, ERROR_INT_STATUS, err);
    }
}

pub fn is_initialized() -> bool {
    unsafe { (*(&raw const G_SDHCI)).initialized }
}

pub fn base_addr() -> usize {
    unsafe { (*(&raw const G_SDHCI)).base }
}
