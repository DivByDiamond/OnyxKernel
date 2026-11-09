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
