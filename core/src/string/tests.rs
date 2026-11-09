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
    let a = b"abc\0";
    let b = b"abd\0";
    unsafe {
        assert_eq!(strcmp(a.as_ptr(), a.as_ptr()), 0);
        assert!(strcmp(a.as_ptr(), b.as_ptr()) < 0);
    }
}
#[test]
fn test_strncmp() {
    let a = b"abcdef\0";
    let b = b"abcxyz\0";
    unsafe {
        assert_eq!(strncmp(a.as_ptr(), b.as_ptr(), 3), 0);
        let r = strncmp(a.as_ptr(), b.as_ptr(), 4);
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
