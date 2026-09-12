//! Kernel errno rendering helper shared by both `passwd` flows.

use onyx_init::syscalls;

/// Writes a negative kernel errno as a decimal string followed by a
/// newline, e.g. `-1\n`. Used to surface real syscall failures instead of
/// silently discarding them as a generic "unknown" condition.
pub unsafe fn write_errno(errno: i64) {
    unsafe {
        let mut buf = [0u8; 21];
        let mut i = buf.len();
        let mut n = if errno < 0 { -errno } else { errno } as u64;
        loop {
            i -= 1;
            buf[i] = b'0' + (n % 10) as u8;
            n /= 10;
            if n == 0 {
                break;
            }
        }
        if errno < 0 {
            i -= 1;
            buf[i] = b'-';
        }
        syscalls::write(1, buf[i..].as_ptr(), buf.len() - i);
        syscalls::write(1, b"\n".as_ptr(), b"\n".len());
    }
}
