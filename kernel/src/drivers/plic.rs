//! PLIC driver — context = 2*hart+1 (S-mode), IRQ dispatch.
use crate::arch::mmio::Mmio;
use crate::arch::regs::*;

static mut G_BASE: u64 = PLIC_BASE;
const HART0_SMODE_CTX: usize = 1;

pub type IrqHandler = fn();
const MAX_IRQ: usize = 64;
static mut G_HANDLERS: [Option<IrqHandler>; MAX_IRQ] = [None; MAX_IRQ];

pub unsafe fn init(base: u64) {
    unsafe {
        G_BASE = base;
    }
}

pub unsafe fn register_handler(irq: u32, handler: IrqHandler) {
    unsafe {
        if (irq as usize) < MAX_IRQ {
            G_HANDLERS[irq as usize] = Some(handler);
        }
    }
}

pub unsafe fn dispatch() {
    unsafe {
        let irq = claim();
        if irq == 0 {
            return;
        }
        if (irq as usize) < MAX_IRQ
            && let Some(h) = (G_HANDLERS)[irq as usize]
        {
            h();
        }
        complete(irq);
    }
}

pub unsafe fn set_priority(irq: u32, prio: u32) {
    unsafe {
        Mmio::<u32>::at(((G_BASE) + 4 * irq as u64) as usize).write(prio & 7);
    }
}

pub unsafe fn enable(irq: u32, hart: usize) {
    unsafe {
        let ctx = plic_smode_ctx(hart);
        let addr = ((G_BASE) + 0x2000 + 0x80 * ctx as u64 + 4 * (irq as u64 / 32)) as usize;
        let m = Mmio::<u32>::at(addr);
        m.write(m.read() | (1u32 << (irq % 32)));
    }
}

pub unsafe fn set_threshold(t: u32) {
    unsafe {
        Mmio::<u32>::at(((G_BASE) + 0x20_0000 + 0x1000 * HART0_SMODE_CTX as u64) as usize)
            .write(t & 7);
    }
}

pub unsafe fn claim() -> u32 {
    unsafe {
        Mmio::<u32>::at(((G_BASE) + 0x20_0004 + 0x1000 * HART0_SMODE_CTX as u64) as usize).read()
    }
}

pub unsafe fn complete(irq: u32) {
    unsafe {
        Mmio::<u32>::at(((G_BASE) + 0x20_0004 + 0x1000 * HART0_SMODE_CTX as u64) as usize)
            .write(irq);
    }
}
