//! NS16550A UART driver.
use crate::arch::mmio::MmioBlock;
pub const R_DATA: u32 = 0;
pub const R_IER: u32 = 1;
pub const R_IIR_FCR: u32 = 2;
pub const R_LCR: u32 = 3;
pub const R_MCR: u32 = 4;
pub const R_LSR: u32 = 5;
pub const LSR_THRE: u8 = 0x20;
pub const LSR_DR: u8 = 0x01;
// Interrupt enable register (IER) bits. Only "received data available"
// is used: the driver is poll-free on the input side but leaves TX on the
// THRE-spin path in `putc`.
pub const IER_RX: u8 = 0x01;
// FCR (shared with IIR at offset 2): FIFO control.
pub const FCR_FIFO_ENA: u8 = 0x01;
pub const FCR_RX_FIFO_RESET: u8 = 0x02;
pub const FCR_TX_FIFO_RESET: u8 = 0x04;
// Bits 6-7 = RX trigger level. 0b00 = 1 byte: with RX interrupts enabled
// (`enable_rx_irq` + the PLIC handler in `drivers::uart_rx`) this wakes the
// handler on every keystroke instead of waiting for a 14-byte burst or the
// FIFO character-timeout interrupt.
pub const FCR_TRIGGER_1: u8 = 0x00;
static mut G_UART: Uart = Uart::new();

#[derive(Clone, Copy)]
pub struct Uart {
    base: usize,
    shift: u32,
}
impl Default for Uart {
    fn default() -> Self {
        Self::new()
    }
}

impl Uart {
    pub const fn new() -> Self {
        Self {
            base: 0x1000_0000,
            shift: 0,
        }
    }
    pub const fn with_config(base: usize, shift: u32) -> Self {
        Self { base, shift }
    }
    fn regs(self) -> MmioBlock {
        MmioBlock::new(self.base, self.shift)
    }
    pub fn init(self, base: usize, shift: u32) {
        let uart = Self::with_config(base, shift);
        // SAFETY: base/shift describe the UART MMIO window from the FDT node or QEMU virt default; writes target NS16550A register offsets (R_*).
        unsafe {
            let r = uart.regs();
            r.reg_u8(R_IER).write(0x00);
            r.reg_u8(R_LCR).write(0x80);
            r.reg_u8(R_DATA).write(0x01);
            r.reg_u8(R_IER).write(0x00);
            r.reg_u8(R_LCR).write(0x03);
            r.reg_u8(R_IIR_FCR)
                .write(FCR_FIFO_ENA | FCR_RX_FIFO_RESET | FCR_TX_FIFO_RESET | FCR_TRIGGER_1);
            r.reg_u8(R_MCR).write(0x0B);
        }
        // SAFETY: publishing the configured UART during single-threaded boot console init; kernel code never runs with SIE set (see crate::sync).
        unsafe {
            G_UART = uart;
        }
    }
    pub fn putc(self, c: u8) {
        // SAFETY: self.base is the configured UART MMIO base; LSR poll and DATA write use NS16550A register offsets via MmioBlock.
        unsafe {
            let r = self.regs();
            // Bounded wait for THRE, then write regardless. sedna's UART
            // accepts the byte into its transmit FIFO even while the
            // transmitter is busy, so an unbounded poll would stall the
            // kernel (QEMU sets THRE instantly; a wrong console address must
            // not hang the boot either).
            let mut spins = 0u32;
            while r.reg_u8(R_LSR).read() & LSR_THRE == 0 {
                spins += 1;
                if spins > 0x4000 {
                    break;
                }
            }
            r.reg_u8(R_DATA).write(c);
        }
    }
    pub fn puts(self, s: &str) {
        for &b in s.as_bytes() {
            if b == b'\n' {
                self.putc(b'\r');
            }
            self.putc(b);
        }
    }
    /// Enable the "received data available" interrupt (IER bit 0).
    ///
    /// `init` deliberately leaves IER at 0 until the PLIC handler is
    /// registered, so no interrupt can be raised with nothing to service it.
    pub fn enable_rx_irq(self) {
        // SAFETY: self.base is the configured UART MMIO base; IER is a
        // write-only NS16550A register at offset R_IER.
        unsafe {
            self.regs().reg_u8(R_IER).write(IER_RX);
        }
    }

    /// One raw byte from the hardware FIFO, bypassing the software ring.
    pub fn read_fifo_byte(self) -> Option<u8> {
        // SAFETY: self.base is the configured UART MMIO base; LSR poll and DATA read use NS16550A register offsets via MmioBlock.
        unsafe {
            let r = self.regs();
            if r.reg_u8(R_LSR).read() & LSR_DR != 0 {
                Some(r.reg_u8(R_DATA).read())
            } else {
                None
            }
        }
    }

    /// Next input byte: the interrupt-fed ring first, then the hardware
    /// FIFO directly. The fallback keeps early-boot input (before the PLIC
    /// handler exists) and platforms without a usable UART IRQ working.
    pub fn getc(self) -> Option<u8> {
        super::uart_rx::pop().or_else(|| self.read_fifo_byte())
    }
    /// Non-consuming readiness peek: true when at least one RX byte sits in
    /// the hardware FIFO (LSR.DR set). Unlike getc() this does not pop the
    /// byte, so poll()/FIONREAD can probe stdin without stealing input from
    /// a concurrent reader.
    pub fn rx_ready(self) -> bool {
        if super::uart_rx::has_data() {
            return true;
        }
        // SAFETY: self.base is the configured UART MMIO base; LSR read uses the NS16550A register offset via MmioBlock.
        unsafe {
            let r = self.regs();
            r.reg_u8(R_LSR).read() & LSR_DR != 0
        }
    }
    pub fn base(self) -> usize {
        self.base
    }
}

pub fn init(base: usize, shift: u32) {
    crate::srv::klog::debug_mark(b'u');
    // SAFETY: boot-time single-threaded console bring-up; G_UART is accessed only from kernel context, which never runs with SIE set (see crate::sync).
    unsafe {
        G_UART.init(base, shift);
    }
}
pub fn init_default() {
    init(0x1000_0000, 0);
}
pub fn putc(c: u8) {
    // SAFETY: G_UART defaults to the QEMU virt base (0x1000_0000) and is reconfigured by uart::init during single-threaded boot; NS16550A register accesses from kernel context, which never runs with SIE set (see crate::sync).
    unsafe {
        let p = &raw const G_UART;
        (*p).putc(c);
    }
}
pub fn puts(s: &str) {
    // SAFETY: G_UART defaults to the QEMU virt base (0x1000_0000) and is reconfigured by uart::init during single-threaded boot; NS16550A register accesses from kernel context, which never runs with SIE set (see crate::sync).
    unsafe {
        let p = &raw const G_UART;
        (*p).puts(s);
    }
}
pub fn getc() -> Option<u8> {
    // SAFETY: G_UART defaults to the QEMU virt base (0x1000_0000) and is reconfigured by uart::init during single-threaded boot; NS16550A register accesses from kernel context, which never runs with SIE set (see crate::sync).
    unsafe {
        let p = &raw const G_UART;
        (*p).getc()
    }
}
/// Enable the UART RX interrupt on the configured console.
pub fn enable_rx_irq() {
    // SAFETY: G_UART is written once at boot (uart::init) and only read
    // afterwards; the IER write touches the configured MMIO base.
    unsafe {
        let p = &raw const G_UART;
        (*p).enable_rx_irq();
    }
}

/// One raw FIFO byte, bypassing the software ring (IRQ handler path).
pub fn read_fifo_byte() -> Option<u8> {
    // SAFETY: G_UART is written once at boot and only read afterwards; MMIO
    // access uses the configured base.
    unsafe {
        let p = &raw const G_UART;
        (*p).read_fifo_byte()
    }
}

/// Buffered RX bytes awaiting a reader (0 when only the FIFO has data).
pub fn rx_buffered() -> usize {
    super::uart_rx::available()
}

/// Non-consuming stdin readiness peek (see Uart::rx_ready).
pub fn rx_ready() -> bool {
    // SAFETY: G_UART defaults to the QEMU virt base (0x1000_0000) and is reconfigured by uart::init during single-threaded boot; NS16550A register accesses from kernel context, which never runs with SIE set (see crate::sync).
    unsafe {
        let p = &raw const G_UART;
        (*p).rx_ready()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_offsets() {
        assert_eq!(R_DATA, 0);
        assert_eq!(R_IER, 1);
        assert_eq!(R_IIR_FCR, 2);
        assert_eq!(R_LCR, 3);
        assert_eq!(R_MCR, 4);
        assert_eq!(R_LSR, 5);
    }

    #[test]
    fn test_lsr_flags() {
        assert_eq!(LSR_THRE, 0x20);
        assert_eq!(LSR_DR, 0x01);
    }

    #[test]
    fn test_fcr_fifo_trigger1() {
        let fcr = FCR_FIFO_ENA | FCR_RX_FIFO_RESET | FCR_TX_FIFO_RESET | FCR_TRIGGER_1;
        assert_eq!(fcr, 0x07);
        assert_eq!(IER_RX & LSR_DR, 1);
    }

    #[test]
    fn test_uart_new_default() {
        let u = Uart::new();
        assert_eq!(u.base(), 0x1000_0000);
    }

    #[test]
    fn test_uart_with_config() {
        let u = Uart::with_config(0x1000_1000, 2);
        assert_eq!(u.base(), 0x1000_1000);
    }

    #[test]
    fn test_uart_shift() {
        let u0 = Uart::with_config(0x1000_0000, 0);
        let u2 = Uart::with_config(0x1000_0000, 2);
        assert_eq!(u0.base(), u2.base());
    }

    #[test]
    fn test_uart_size() {
        assert_eq!(core::mem::size_of::<Uart>(), 16);
    }
}
