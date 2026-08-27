//! Low-level C-style string and memory primitives.

/// # Safety
///
/// `s` must point to a valid NUL-terminated byte string.
pub unsafe fn strlen(s: *const u8) -> usize {
    unsafe {
        let mut n = 0;
        while *s.add(n) != 0 {
            n += 1;
        }
        n
    }
}
/// # Safety
///
/// `a` and `b` must each point to a valid NUL-terminated byte string.
pub unsafe fn strcmp(a: *const u8, b: *const u8) -> i32 {
    unsafe {
        let mut i = 0;
        loop {
            let ca = *a.add(i);
            let cb = *b.add(i);
            if ca != cb {
                return i32::from(ca) - i32::from(cb);
            }
            if ca == 0 {
                return 0;
            }
            i += 1;
        }
    }
}
/// # Safety
///
/// `a` and `b` must be valid for reads of `n` bytes.
pub unsafe fn strncmp(mut a: *const u8, mut b: *const u8, mut n: usize) -> i32 {
    unsafe {
        while n > 0 {
            let ca = *a;
            let cb = *b;
            if ca != cb {
                return i32::from(ca) - i32::from(cb);
            }
            if ca == 0 {
                return 0;
            }
            a = a.add(1);
            b = b.add(1);
            n -= 1;
        }
        0
    }
}
/// # Safety
///
/// `dst` and `src` must be valid for reads/writes of `n` bytes and must not
/// overlap.
pub unsafe fn memcpy(dst: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    unsafe {
        let mut i = 0;
        while i < n {
            *dst.add(i) = *src.add(i);
            i += 1;
        }
        dst
    }
}
/// # Safety
///
/// `s` must be valid for writes of `n` bytes.
pub unsafe fn memset(s: *mut u8, c: u8, n: usize) -> *mut u8 {
    unsafe {
        let mut i = 0;
        while i < n {
            *s.add(i) = c;
            i += 1;
        }
        s
    }
}
/// # Safety
///
/// `dst` and `src` must be valid for reads/writes of `n` bytes; overlap is
/// handled like C `memmove`.
pub unsafe fn memmove(dst: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    unsafe {
        match dst.cmp(&(src as *mut u8)) {
            core::cmp::Ordering::Less => memcpy(dst, src, n),
            core::cmp::Ordering::Greater => {
                let mut i = n;
                while i > 0 {
                    i -= 1;
                    *dst.add(i) = *src.add(i);
                }
                dst
            }
            core::cmp::Ordering::Equal => dst,
        }
    }
}

/// # Safety
///
/// Same contract as [`memcpy`]; `d`, `s` valid for `n` bytes, no overlap.
#[cfg(not(test))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rust_memcpy(d: *mut u8, s: *const u8, n: usize) -> *mut u8 {
    unsafe { memcpy(d, s, n) }
}
/// # Safety
///
/// Same contract as [`memset`]; `s` valid for `n` bytes.
#[cfg(not(test))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rust_memset(s: *mut u8, c: u8, n: usize) -> *mut u8 {
    unsafe { memset(s, c, n) }
}
/// # Safety
///
/// Same contract as [`memmove`]; `d`, `s` valid for `n` bytes.
#[cfg(not(test))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rust_memmove(d: *mut u8, s: *const u8, n: usize) -> *mut u8 {
    unsafe { memmove(d, s, n) }
}

#[cfg(all(test, not(target_os = "none")))]
mod tests {
    use super::*;
    #[test]
    fn test_strlen() {
        let s = b"hello\0";
        unsafe {
            assert_eq!(strlen(s.as_ptr()), 5);
        }
    }
    #[test]
    fn test_strcmp() {
        unsafe {
            assert_eq!(strcmp(b"abc\0".as_ptr(), b"abc\0".as_ptr()), 0);
            assert!(strcmp(b"abc\0".as_ptr(), b"abd\0".as_ptr()) < 0);
        }
    }
    #[test]
    fn test_strncmp() {
        unsafe {
            assert_eq!(strncmp(b"abcdef\0".as_ptr(), b"abcxyz\0".as_ptr(), 3), 0);
            let r = strncmp(b"abcdef\0".as_ptr(), b"abcxyz\0".as_ptr(), 4);
            assert!(r < 0);
        }
    }
    #[test]
    fn test_memcpy() {
        let src = b"hello world";
        let mut dst = [0u8; 11];
        unsafe {
            memcpy(dst.as_mut_ptr(), src.as_ptr(), 11);
        }
        assert_eq!(&dst, src);
    }
    #[test]
    fn test_memset() {
        let mut buf = [0u8; 8];
        unsafe {
            memset(buf.as_mut_ptr(), 0xAA, 8);
        }
        assert_eq!(buf, [0xAA; 8]);
    }
    #[test]
    fn test_memmove() {
        let mut buf = *b"abcdefg";
        unsafe {
            memmove(buf.as_mut_ptr().add(2), buf.as_ptr(), 5);
        }
        assert_eq!(&buf, b"ababcde");
    }
}
