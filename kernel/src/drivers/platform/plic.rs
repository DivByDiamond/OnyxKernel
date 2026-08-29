//! PLIC driver — context = 2*hart+1 (S-mode), IRQ dispatch.
use crate::arch::mmio::Mmio;
use crate::arch::regs::*;

static mut G_BASE: u64 = PLIC_BASE;
const HART0_SMODE_CTX: usize = 1;

pub type IrqHandler = fn();
const MAX_IRQ: usize = 64;
static mut G_HANDLERS: [Option<IrqHandler>; MAX_IRQ] = [None; MAX_IRQ];

/// # Safety
///
/// Caller contract: `base` must be the validated PLIC MMIO base (from the
/// FDT node or a fixed SoC constant) and `init` must run once, on a single
/// hart, before any PLIC access.
pub unsafe fn init(base: u64) {
    // SAFETY: single-threaded init; G_BASE is only written here before harts use the PLIC.
    unsafe {
        G_BASE = base;
    }
}

/// # Safety
///
/// Caller contract: `irq < MAX_IRQ` is enforced internally; callers must not
/// race registration against dispatch (single-threaded init / SIE=0).
pub unsafe fn register_handler(irq: u32, handler: IrqHandler) {
    // SAFETY: access to G_HANDLERS is IRQ-index bounds-checked; no concurrent mutation (SIE=0 during init).
    unsafe {
        if (irq as usize) < MAX_IRQ {
            G_HANDLERS[irq as usize] = Some(handler);
        }
    }
}

/// # Safety
///
/// Caller contract: must run on the hart owning the S-mode context, with
/// handlers already registered and `irq` indices below MAX_IRQ.
pub unsafe fn dispatch() {
    // SAFETY: G_HANDLERS access is bounds-checked against MAX_IRQ; claim/complete target the valid PLIC context.
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

/// # Safety
///
/// Caller contract: G_BASE must be initialised to a valid PLIC base and
/// `irq` must be a real PLIC source id (< 1024 per PLIC spec).
pub unsafe fn set_priority(irq: u32, prio: u32) {
    // SAFETY: G_BASE is a device base validated at init; the priority register offset 4*irq lies in the PLIC MMIO window (identity-mapped).
    unsafe {
        Mmio::<u32>::at(((G_BASE) + 4 * irq as u64) as usize).write(prio & 7);
    }
}

/// # Safety
///
/// Caller contract: G_BASE must be a valid PLIC base; `hart` must be a
/// valid hart index whose S-mode context exists on this platform.
pub unsafe fn enable(irq: u32, hart: usize) {
    // SAFETY: enables-register offset derived from G_BASE (validated at init) plus per-context/per-word datasheet constants; within the PLIC MMIO window.
    unsafe {
        let ctx = plic_smode_ctx(hart);
        let addr = ((G_BASE) + 0x2000 + 0x80 * ctx as u64 + 4 * (irq as u64 / 32)) as usize;
        let m = Mmio::<u32>::at(addr);
        m.write(m.read() | (1u32 << (irq % 32)));
    }
}

/// # Safety
///
/// Caller contract: G_BASE must be a valid PLIC base.
pub unsafe fn set_threshold(t: u32) {
    // SAFETY: threshold register offset is a fixed datasheet constant from the validated G_BASE; within the PLIC MMIO window.
    unsafe {
        Mmio::<u32>::at(((G_BASE) + 0x20_0000 + 0x1000 * HART0_SMODE_CTX as u64) as usize)
            .write(t & 7);
    }
}

/// # Safety
///
/// Caller contract: G_BASE must be a valid PLIC base; only call from the
/// hart that owns the S-mode context (claim/complete are hart-local).
pub unsafe fn claim() -> u32 {
    // SAFETY: claim register offset is a fixed datasheet constant from the validated G_BASE; within the PLIC MMIO window.
    unsafe {
        Mmio::<u32>::at(((G_BASE) + 0x20_0004 + 0x1000 * HART0_SMODE_CTX as u64) as usize).read()
    }
}

/// # Safety
///
/// Caller contract: `irq` must be a value previously returned by `claim`
/// on this context and G_BASE must be a valid PLIC base.
pub unsafe fn complete(irq: u32) {
    // SAFETY: complete register offset is a fixed datasheet constant from the validated G_BASE; within the PLIC MMIO window.
    unsafe {
        Mmio::<u32>::at(((G_BASE) + 0x20_0004 + 0x1000 * HART0_SMODE_CTX as u64) as usize)
            .write(irq);
    }
}
