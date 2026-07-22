//! NS16550A UART driver.
use crate::arch::mmio::{Mmio, MmioBlock};
pub const R_DATA: u32 = 0;
pub const R_IER: u32 = 1;
pub const R_IIR_FCR: u32 = 2;
pub const R_LCR: u32 = 3;
pub const R_MCR: u32 = 4;
pub const R_LSR: u32 = 5;
pub const LSR_THRE: u8 = 0x20;
pub const LSR_DR: u8 = 0x01;
static mut G_UART: Uart = Uart::new();

#[derive(Clone, Copy)]
pub struct Uart {
    base: usize,
    shift: u32,
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
        unsafe {
            let r = uart.regs();
            r.reg_u8(R_IER).write(0x00);
            r.reg_u8(R_LCR).write(0x80);
            r.reg_u8(R_DATA).write(0x01);
            r.reg_u8(R_IER).write(0x00);
            r.reg_u8(R_LCR).write(0x03);
            r.reg_u8(R_IIR_FCR).write(0xC7);
            r.reg_u8(R_MCR).write(0x0B);
        }
        unsafe {
            G_UART = uart;
        }
    }
    pub fn putc(self, c: u8) {
        unsafe {
            let r = self.regs();
            while r.reg_u8(R_LSR).read() & LSR_THRE == 0 {}
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
    pub fn getc(self) -> Option<u8> {
        unsafe {
            let r = self.regs();
            if r.reg_u8(R_LSR).read() & LSR_DR != 0 {
                Some(r.reg_u8(R_DATA).read())
            } else {
                None
            }
        }
    }
    pub fn base(self) -> usize {
        self.base
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

pub fn init(base: usize, shift: u32) {
    unsafe {
        G_UART.init(base, shift);
    }
}
pub fn init_default() {
    init(0x1000_0000, 0);
}
pub fn putc(c: u8) {
    unsafe {
        let p = &raw const G_UART;
        (*p).putc(c);
    }
}
pub fn puts(s: &str) {
    unsafe {
        let p = &raw const G_UART;
        (*p).puts(s);
    }
}
pub fn getc() -> Option<u8> {
    unsafe {
        let p = &raw const G_UART;
        (*p).getc()
    }
}
