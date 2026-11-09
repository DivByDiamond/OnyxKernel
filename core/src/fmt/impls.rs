//! Arg enum + From impls.

#[derive(Clone, Copy)]
pub enum Arg<'a> {
    Str(&'a str),
    CStr(*const u8),
    Char(u8),
    I64(i64),
    U64(u64),
    ISize(isize),
    USize(usize),
}

impl<'a> From<&'a str> for Arg<'a> {
    fn from(s: &'a str) -> Self {
        Self::Str(s)
    }
}
impl<'a> From<&'a [u8]> for Arg<'a> {
    fn from(s: &'a [u8]) -> Self {
        Self::Str(core::str::from_utf8(s).unwrap_or("?"))
    }
}
impl From<i32> for Arg<'_> {
    fn from(v: i32) -> Self {
        Self::I64(i64::from(v))
    }
}
impl From<i64> for Arg<'_> {
    fn from(v: i64) -> Self {
        Self::I64(v)
    }
}
impl From<u32> for Arg<'_> {
    fn from(v: u32) -> Self {
        Self::U64(u64::from(v))
    }
}
impl From<u64> for Arg<'_> {
    fn from(v: u64) -> Self {
        Self::U64(v)
    }
}
impl From<usize> for Arg<'_> {
    fn from(v: usize) -> Self {
        Self::USize(v)
    }
}
impl From<u8> for Arg<'_> {
    fn from(v: u8) -> Self {
        Self::Char(v)
    }
}

#[cfg(all(test, not(target_os = "none")))]
mod tests;
