#![expect(dead_code)]

use crate::auth::crypto::{
    bytes_to_hex, const_time_eq, generate_salt, hash_password, hex_decode_8,
};
use crate::auth::SHADOW_PATH;
use crate::syscalls;

pub fn read_shadow_password(username: &[u8]) -> Result<[u8; 128], i64> {
    let mut path_buf = [0u8; 64];
    let n = SHADOW_PATH.len().min(63);
    path_buf[..n].copy_from_slice(&SHADOW_PATH[..n]);
    let fd = unsafe { syscalls::open(path_buf.as_ptr(), 0, 0) };
    if fd < 0 {
        return Err(fd);
    }
    let mut buf = [0u8; 4096];
    let mut total = 0usize;
    loop {
        let n = unsafe {
            syscalls::read(
                fd as u64,
                buf[total..].as_mut_ptr(),
                (buf.len() - total) as u64,
            )
        };
        if n <= 0 {
            break;
        }
        total += n as usize;
        if total >= buf.len() {
            break;
        }
    }
    unsafe { syscalls::close(fd as u64) };

    let mut shadow_val = [0u8; 128];
    let data = &buf[..total];
    let mut pos = 0;
    while pos < data.len() {
        let line_end = match data[pos..].iter().position(|&b| b == b'\n') {
            Some(n) => pos + n,
            None => data.len(),
        };
        let line = &data[pos..line_end];
        pos = line_end + 1;

        let colon = match line.iter().position(|&b| b == b':') {
            Some(n) => n,
            None => continue,
        };
        let name = &line[..colon];
        let entry = &line[colon + 1..];

        if name.len() == username.len() && name == username {
            let n = entry.len().min(127);
            shadow_val[..n].copy_from_slice(&entry[..n]);
            return Ok(shadow_val);
        }
    }
    Err(-2)
}

pub fn verify_shadow_password(username: &[u8], password: &[u8]) -> bool {
    let stored = match read_shadow_password(username) {
        Ok(s) => s,
        Err(_) => return false,
    };

    let stored_len = stored.iter().position(|&b| b == 0).unwrap_or(stored.len());
    let data = &stored[..stored_len];

    if data.len() < 3 || data[0] != b'$' || data[1] != b'5' || data[2] != b'$' {
        return false;
    }

    let rest = &data[3..];
    let salt_end = match rest.iter().position(|&b| b == b'$') {
        Some(n) => n,
        None => return false,
    };
    let salt_hex = &rest[..salt_end];
    let stored_hash_hex = &rest[salt_end + 1..];

    if salt_hex.len() != 16 || stored_hash_hex.len() < 64 {
        return false;
    }
    let stored_hash_hex = &stored_hash_hex[..64];

    let salt_bytes = hex_decode_8(salt_hex);

    let computed_hash = hash_password(password, &salt_bytes);
    let computed_hex = bytes_to_hex(&computed_hash);

    const_time_eq(&computed_hex[..64], stored_hash_hex)
}

pub(crate) fn format_shadow_entry(username: &[u8], password: &[u8]) -> ([u8; 128], usize) {
    let salt = generate_salt();
    let hash = hash_password(password, &salt);
    let salt_hex = bytes_to_hex(&salt);
    let hash_hex = bytes_to_hex(&hash);

    let mut buf = [0u8; 128];
    let mut pos = 0;

    for &b in username {
        if pos >= buf.len() {
            break;
        }
        buf[pos] = b;
        pos += 1;
    }
    if pos < buf.len() {
        buf[pos] = b':';
        pos += 1;
    }
    for &b in b"$5$" {
        if pos >= buf.len() {
            break;
        }
        buf[pos] = b;
        pos += 1;
    }
    for i in 0..16 {
        if pos >= buf.len() {
            break;
        }
        buf[pos] = salt_hex[i];
        pos += 1;
    }
    if pos < buf.len() {
        buf[pos] = b'$';
        pos += 1;
    }
    for i in 0..64 {
        if pos >= buf.len() {
            break;
        }
        buf[pos] = hash_hex[i];
        pos += 1;
    }
    (buf, pos)
}
