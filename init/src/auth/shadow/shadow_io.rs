#![expect(dead_code)]

use super::shadow_core::format_shadow_entry;
use crate::auth::group::atomic_rewrite;
use crate::auth::SHADOW_PATH;
use crate::syscalls;

pub fn update_shadow_password(username: &[u8], new_password: &[u8]) -> Result<(), i64> {
    let mut path_buf = [0u8; 64];
    let n = SHADOW_PATH.len().min(63);
    path_buf[..n].copy_from_slice(&SHADOW_PATH[..n]);

    let mut buf = [0u8; 4096];
    let mut total = 0usize;

    let fd = unsafe { syscalls::open(path_buf.as_ptr(), 0, 0) };
    if fd >= 0 {
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
    }

    let mut out = [0u8; 4096];
    let mut out_pos = 0;
    let data = &buf[..total];
    let mut found = false;
    let mut data_pos = 0;

    while data_pos < data.len() {
        let line_end = match data[data_pos..].iter().position(|&b| b == b'\n') {
            Some(n) => data_pos + n,
            None => data.len(),
        };
        let line = &data[data_pos..line_end];
        data_pos = line_end + 1;

        let colon = match line.iter().position(|&b| b == b':') {
            Some(n) => n,
            None => {
                let copy_end = (out_pos + line.len()).min(out.len());
                let to_copy = copy_end - out_pos;
                out[out_pos..copy_end].copy_from_slice(&line[..to_copy]);
                out_pos = copy_end;
                if out_pos < out.len() {
                    out[out_pos] = b'\n';
                    out_pos += 1;
                }
                continue;
            }
        };
        let name = &line[..colon];

        if name == username {
            let (entry, entry_len) = format_shadow_entry(username, new_password);
            let copy_end = (out_pos + entry_len).min(out.len());
            let to_copy = copy_end - out_pos;
            out[out_pos..copy_end].copy_from_slice(&entry[..to_copy]);
            out_pos = copy_end;
            if out_pos < out.len() {
                out[out_pos] = b'\n';
                out_pos += 1;
            }
            found = true;
        } else {
            let copy_end = (out_pos + line.len()).min(out.len());
            let to_copy = copy_end - out_pos;
            out[out_pos..copy_end].copy_from_slice(&line[..to_copy]);
            out_pos = copy_end;
            if out_pos < out.len() {
                out[out_pos] = b'\n';
                out_pos += 1;
            }
        }
    }

    if !found {
        let (entry, entry_len) = format_shadow_entry(username, new_password);
        let copy_end = (out_pos + entry_len).min(out.len());
        let to_copy = copy_end - out_pos;
        out[out_pos..copy_end].copy_from_slice(&entry[..to_copy]);
        out_pos = copy_end;
        if out_pos < out.len() {
            out[out_pos] = b'\n';
            out_pos += 1;
        }
    }

    atomic_rewrite(&path_buf, n, &out[..out_pos], 0o600)
}

pub fn delete_shadow_entry(username: &[u8]) -> Result<(), i64> {
    let mut path_buf = [0u8; 64];
    let n = SHADOW_PATH.len().min(63);
    path_buf[..n].copy_from_slice(&SHADOW_PATH[..n]);

    let mut buf = [0u8; 4096];
    let mut total = 0usize;

    let fd = unsafe { syscalls::open(path_buf.as_ptr(), 0, 0) };
    if fd < 0 {
        return Err(fd);
    }
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

    let mut out = [0u8; 4096];
    let mut out_pos = 0;
    let data = &buf[..total];
    let mut data_pos = 0;

    while data_pos < data.len() {
        let line_end = match data[data_pos..].iter().position(|&b| b == b'\n') {
            Some(n) => data_pos + n,
            None => data.len(),
        };
        let line = &data[data_pos..line_end];
        data_pos = line_end + 1;

        let colon = match line.iter().position(|&b| b == b':') {
            Some(n) => n,
            None => continue,
        };
        let name = &line[..colon];

        if name == username {
            continue;
        }

        let copy_end = (out_pos + line.len()).min(out.len());
        let to_copy = copy_end - out_pos;
        out[out_pos..copy_end].copy_from_slice(&line[..to_copy]);
        out_pos = copy_end;
        if out_pos < out.len() {
            out[out_pos] = b'\n';
            out_pos += 1;
        }
    }

    atomic_rewrite(&path_buf, n, &out[..out_pos], 0o600)
}
