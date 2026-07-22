#![expect(dead_code)]

use super::passwd_core::{format_passwd_entry, parse_passwd, PasswdEntry};
use crate::auth::group::atomic_rewrite;
use crate::auth::PASSWD_PATH;
use crate::syscalls;

pub fn read_passwd(users: &mut [PasswdEntry; crate::auth::MAX_USERS]) -> Result<usize, i64> {
    let mut path_buf = [0u8; 64];
    let n = PASSWD_PATH.len().min(63);
    path_buf[..n].copy_from_slice(&PASSWD_PATH[..n]);
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

    Ok(parse_passwd(&buf[..total], users))
}

pub fn update_passwd_entry(
    username: &[u8],
    uid: u32,
    gid: u32,
    home: &[u8],
    shell: &[u8],
) -> Result<(), i64> {
    let mut path_buf = [0u8; 64];
    let n = PASSWD_PATH.len().min(63);
    path_buf[..n].copy_from_slice(&PASSWD_PATH[..n]);

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
            None => continue,
        };
        let name = &line[..colon];

        if name == username {
            let (entry, entry_len) = format_passwd_entry(username, uid, gid, home, shell);
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
        let (entry, entry_len) = format_passwd_entry(username, uid, gid, home, shell);
        let copy_end = (out_pos + entry_len).min(out.len());
        let to_copy = copy_end - out_pos;
        out[out_pos..copy_end].copy_from_slice(&entry[..to_copy]);
        out_pos = copy_end;
        if out_pos < out.len() {
            out[out_pos] = b'\n';
            out_pos += 1;
        }
    }

    atomic_rewrite(&path_buf, n, &out[..out_pos], 0o644)
}

pub fn delete_passwd_entry(username: &[u8]) -> Result<(), i64> {
    let mut path_buf = [0u8; 64];
    let n = PASSWD_PATH.len().min(63);
    path_buf[..n].copy_from_slice(&PASSWD_PATH[..n]);

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

    atomic_rewrite(&path_buf, n, &out[..out_pos], 0o644)
}
