#![expect(dead_code)]

use super::sha256::sha256;
use crate::syscalls;

pub fn bytes_to_hex(bytes: &[u8]) -> [u8; 64] {
    let mut out = [0u8; 64];
    let hex_chars = b"0123456789abcdef";
    let n = bytes.len().min(32);
    for i in 0..n {
        out[i * 2] = hex_chars[(bytes[i] >> 4) as usize];
        out[i * 2 + 1] = hex_chars[(bytes[i] & 0xF) as usize];
    }
    out
}

fn hex_val(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        b'A'..=b'F' => c - b'A' + 10,
        _ => 0,
    }
}

pub(crate) fn hex_decode_8(hex: &[u8]) -> [u8; 8] {
    let mut out = [0u8; 8];
    let n = (hex.len() / 2).min(8);
    for i in 0..n {
        out[i] = (hex_val(hex[i * 2]) << 4) | hex_val(hex[i * 2 + 1]);
    }
    out
}

pub fn const_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut r = 0u8;
    for (ai, bi) in a.iter().zip(b.iter()) {
        r |= ai ^ bi;
    }
    r == 0
}

pub(crate) fn generate_salt() -> [u8; 8] {
    let mut salt = [0u8; 8];
    let r = unsafe { syscalls::getentropy(salt.as_mut_ptr(), 8) };
    if r == 0 {
        return salt;
    }
    let pid = unsafe { syscalls::getpid() } as u64;
    let mut seed = pid
        .wrapping_mul(1103515245)
        .wrapping_add(12345)
        .wrapping_add(r as u64);
    for s in &mut salt {
        seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
        *s = (seed >> 16) as u8;
    }
    salt
}

pub const KDF_ITERS: usize = 10_000;

pub fn hash_password(password: &[u8], salt: &[u8; 8]) -> [u8; 32] {
    let mut h = sha256(password);
    let mut buf = [0u8; 40];
    buf[..32].copy_from_slice(&h);
    buf[32..].copy_from_slice(salt);
    for _ in 0..KDF_ITERS {
        h = sha256(&buf);
        buf[..32].copy_from_slice(&h);
    }
    h
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
