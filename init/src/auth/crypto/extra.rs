// TODO(dead-code): auth::crypto::extra — shared auth/syscalls module, compiled per onyx_init bin;
// items unused by one binary are used by others (dead_code/unused_imports fire per-bin).
#![allow(dead_code, unused_imports)]

use crate::syscalls;

pub use onyx_core::crypto::kdf::{
    KDF_ITERS, bytes_to_hex, const_time_eq, format_shadow_field, hash_password, hex_decode_8,
    legacy_hash_password,
};

static mut WEAK_ENTROPY_WARNED: bool = false;

/// One-time console warning when salt generation has to fall back to a
/// non-cryptographic PRNG. Guarded by a static bool so each userspace
/// process (login/useradd/passwd/su share this module) prints at most one
/// line regardless of how many salts it generates.
///
/// NOTE: the kernel's getentropy always succeeds even when the underlying
/// hwrand source is only an LCG (the "silence contract"), so this warning
/// fires only on genuine syscall failure — the kernel-side degradation
/// signal lives in `kernel/src/drivers/hwrand.rs::strong_source_available`.
fn warn_weak_entropy_once() {
    unsafe {
        if WEAK_ENTROPY_WARNED {
            return;
        }
        WEAK_ENTROPY_WARNED = true;
    }
    const MSG: &[u8] = b"warning: weak entropy source, password salts degraded\n";
    unsafe { syscalls::write(2, MSG.as_ptr(), MSG.len()) };
}

/// Generate an 8-byte salt from `getentropy`. If the syscall fails, degrade
/// to a time/pid-seeded LCG (never fail open by returning early) and emit
/// a single warning line.
pub fn generate_salt() -> [u8; 8] {
    let mut salt = [0u8; 8];
    let r = unsafe { syscalls::getentropy(salt.as_mut_ptr(), 8) };
    if r == 0 {
        return salt;
    }
    warn_weak_entropy_once();
    let pid = unsafe { syscalls::getpid() } as u64;
    let mut ts = [0u64; 2];
    let _ = unsafe { syscalls::clock_gettime(0, ts.as_mut_ptr()) };
    let mut seed = ts[0] ^ ts[1] ^ pid;
    for s in &mut salt {
        seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
        *s = (seed >> 16) as u8;
    }
    salt
}

pub(crate) fn format_dec(n: u32) -> [u8; 12] {
    let mut buf = [0u8; 12];
    let mut pos = 11;
    if n == 0 {
        buf[10] = b'0';
        return buf;
    }
    let mut val = n;
    while val > 0 && pos > 0 {
        pos -= 1;
        buf[pos] = b'0' + (val % 10) as u8;
        val /= 10;
    }
    buf
}

pub(crate) fn parse_dec(s: &[u8]) -> u32 {
    let mut val: u32 = 0;
    for &b in s.iter() {
        if b.is_ascii_digit() {
            val = val.wrapping_mul(10).wrapping_add(u32::from(b - b'0'));
        } else {
            break;
        }
    }
    val
}

pub(crate) fn copy_slice(dst: &mut [u8], src: &[u8]) {
    let n = dst.len().min(src.len());
    dst[..n].copy_from_slice(&src[..n]);
}
