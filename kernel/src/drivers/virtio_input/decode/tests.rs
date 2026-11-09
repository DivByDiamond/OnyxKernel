use super::*;
#[test]
fn evdev_codes_follow_keyboard_rows_and_ascii_digits() {
    for (start, row) in [
        (16, &b"qwertyuiop"[..]),
        (30, &b"asdfghjkl"[..]),
        (44, &b"zxcvbnm"[..]),
    ] {
        for (offset, &letter) in row.iter().enumerate() {
            assert_eq!(
                linux_to_keycode(start + offset as u16),
                KeyCode::Letter(letter)
            );
        }
    }
    for (offset, &digit) in b"1234567890".iter().enumerate() {
        assert_eq!(linux_to_keycode(2 + offset as u16), KeyCode::Digit(digit));
    }
}
#[test]
fn partially_initialized_device_is_not_polled() {
    let mut dev = super::super::EMPTY;
    dev.base = 0x10002000;
    assert!(unsafe { poll_device(&mut dev) }.is_none());
}
