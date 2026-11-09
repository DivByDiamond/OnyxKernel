use super::super::writer::{Write, vformat};
use super::*;

struct Buf {
    s: alloc::string::String,
}
impl Buf {
    fn new() -> Self {
        Self {
            s: alloc::string::String::new(),
        }
    }
}
impl Write for Buf {
    fn write_str(&mut self, s: &str) {
        self.s.push_str(s);
    }
}

#[test]
fn test_simple() {
    let mut b = Buf::new();
    vformat(&mut b, "hello %s!", &[Arg::from("world")]);
    assert_eq!(b.s, "hello world!");
}
#[test]
fn test_dec_pad() {
    let mut b = Buf::new();
    vformat(&mut b, "n=%05d", &[Arg::from(42i32)]);
    assert_eq!(b.s, "n=00042");
}
#[test]
fn test_hex() {
    let mut b = Buf::new();
    vformat(&mut b, "0x%08x", &[Arg::from(0xDEADBEEFu32)]);
    assert_eq!(b.s, "0xdeadbeef");
}
#[test]
fn test_neg() {
    let mut b = Buf::new();
    vformat(&mut b, "%d", &[Arg::from(-123i64)]);
    assert_eq!(b.s, "-123");
}
#[test]
fn test_int64_min() {
    let mut b = Buf::new();
    vformat(&mut b, "%d", &[Arg::from(i64::MIN)]);
    assert_eq!(b.s, "-9223372036854775808");
}
