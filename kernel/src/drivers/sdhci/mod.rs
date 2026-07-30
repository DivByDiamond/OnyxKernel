pub mod block;
pub mod cmd;
pub mod regs;

use self::cmd::*;
use self::regs::*;
use crate::drivers::plic;

pub use self::block::{read, read_multi, write, write_multi};
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

pub(super) unsafe fn wait_idle(base: usize) {
    let mut timeout = SDHCI_TIMEOUT;
    while timeout > 0 {
        let ps = reg_r(base, PRESENT_STATE);
        if (ps & (PS_CMD_INHIBIT | PS_DAT_INHIBIT)) == 0 {
            return;
        }
        timeout -= 1;
    }
}

pub(super) unsafe fn wait_state(base: usize, flag: u32) -> bool {
    let mut timeout = SDHCI_TIMEOUT;
    while timeout > 0 {
        if reg_r(base, PRESENT_STATE) & flag != 0 {
            return true;
        }
        timeout -= 1;
    }
    false
}

pub(super) unsafe fn reset(base: usize, bits: u32) {
    reg_w(base, SOFTWARE_RESET, bits);
    let mut timeout = SDHCI_TIMEOUT;
    while timeout > 0 {
        if reg_r(base, SOFTWARE_RESET) & bits == 0 {
            return;
        }
        timeout -= 1;
    }
}

pub(super) unsafe fn clear_interrupts(base: usize) {
    let norm = reg_r(base, NORMAL_INT_STATUS);
    reg_w(base, NORMAL_INT_STATUS, norm);
    let err = reg_r(base, ERROR_INT_STATUS);
    reg_w(base, ERROR_INT_STATUS, err);
}

pub(super) unsafe fn init_clock(base: usize) {
    let caps0 = reg_r(base, 0x40);
    let base_clk_mhz = ((caps0 >> 8) & 0xFF) as u32;
    let target_mhz: u32 = 50;

    let div: u32 = if base_clk_mhz > 0 && base_clk_mhz > target_mhz {
        let d = base_clk_mhz / (2 * target_mhz);
        if d == 0 { 1 } else { d.min(CLK_MAX_DIV) }
    } else {
        1
    };

    reg_w(base, CLOCK_CONTROL, 0);

    let clk_val = (div << 8) | CLK_INTERNAL_ENABLE;
    reg_w(base, CLOCK_CONTROL, clk_val);

    let mut timeout = SDHCI_TIMEOUT;
    while timeout > 0 {
        if reg_r(base, CLOCK_CONTROL) & CLK_STABLE != 0 {
            break;
        }
        timeout -= 1;
    }

    reg_w(base, CLOCK_CONTROL, clk_val | CLK_SD_CLOCK_ENABLE);
}

pub(super) unsafe fn set_power(base: usize) {
    reg_w(base, POWER_CONTROL, PWR_3_3V);
    let mut delay = 1000u32;
    while delay > 0 {
        delay -= 1;
    }
    reg_w(base, POWER_CONTROL, PWR_3_3V | PWR_BUS_POWER);
}

pub(super) unsafe fn set_sdma_addr(base: usize, addr: u64) {
    reg_w(base, SDMAS_SYS_ADDR, addr as u32);
}

pub(super) unsafe fn wait_transfer_complete(base: usize) -> bool {
    let mut timeout = SDHCI_TIMEOUT;
    while timeout > 0 {
        let istat = reg_r(base, NORMAL_INT_STATUS);
        if istat & INT_ERROR != 0 {
            clear_interrupts(base);
            reset(base, SW_RESET_DAT);
            return false;
        }
        if istat & INT_TRANSFER_COMPLETE != 0 {
            reg_w(base, NORMAL_INT_STATUS, INT_TRANSFER_COMPLETE);
            return true;
        }
        timeout -= 1;
    }
    reset(base, SW_RESET_DAT);
    false
}
