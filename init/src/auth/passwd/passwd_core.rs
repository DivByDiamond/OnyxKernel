#![expect(dead_code)]

use crate::auth::crypto::{copy_slice, format_dec, parse_dec};

#[derive(Clone, Copy)]
pub struct PasswdEntry {
    pub name: [u8; 32],
    pub uid: u32,
    pub gid: u32,
    pub home: [u8; 64],
    pub shell: [u8; 32],
}

pub fn parse_passwd(data: &[u8], users: &mut [PasswdEntry; crate::auth::MAX_USERS]) -> usize {
    let mut count = 0;
    let mut pos = 0;
    while pos < data.len() && count < crate::auth::MAX_USERS {
        let line_end = match data[pos..].iter().position(|&b| b == b'\n') {
            Some(n) => pos + n,
            None => data.len(),
        };
        let line = &data[pos..line_end];
        pos = line_end + 1;

        if line.is_empty() || line[0] == b'#' {
            continue;
        }

        let mut fields = [0usize; 5];
        let mut fi = 0;
        let mut start = 0;
        for (i, &b) in line.iter().enumerate() {
            if b == b':' {
                if fi < fields.len() {
                    fields[fi] = start;
                    fi += 1;
                }
                start = i + 1;
            }
        }
        fields[4] = start;

        if fi < 4 {
            continue;
        }

        let name = &line[fields[0]..fields[1] - 1];
        let uid_str = &line[fields[1]..fields[2] - 1];
        let gid_str = &line[fields[2]..fields[3] - 1];
        let home = &line[fields[3]..fields[4] - 1];
        let shell = &line[fields[4]..];

        let uid = parse_dec(uid_str);
        let gid = parse_dec(gid_str);

        let mut entry = PasswdEntry {
            name: [0; 32],
            uid,
            gid,
            home: [0; 64],
            shell: [0; 32],
        };
        copy_slice(&mut entry.name, name);
        copy_slice(&mut entry.home, home);
        copy_slice(&mut entry.shell, shell);
        users[count] = entry;
        count += 1;
    }
    count
}

pub fn find_user(
    users: &[PasswdEntry; crate::auth::MAX_USERS],
    count: usize,
    name: &[u8],
) -> Option<usize> {
    users[..count].iter().position(|entry| {
        let mut match_len = 0;
        while match_len < entry.name.len() && entry.name[match_len] != 0 && match_len < name.len() {
            if entry.name[match_len] != name[match_len] {
                break;
            }
            match_len += 1;
        }
        match_len == name.len() && (entry.name[match_len] == 0 || match_len == entry.name.len())
    })
}

pub fn find_user_by_uid(
    users: &[PasswdEntry; crate::auth::MAX_USERS],
    count: usize,
    uid: u32,
) -> Option<usize> {
    users[..count].iter().position(|e| e.uid == uid)
}

pub(crate) fn format_passwd_entry(
    username: &[u8],
    uid: u32,
    gid: u32,
    home: &[u8],
    shell: &[u8],
) -> ([u8; 256], usize) {
    let mut buf = [0u8; 256];
    let mut pos = 0;

    for &b in username.iter() {
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

    let uid_str = format_dec(uid);
    for &b in uid_str.iter() {
        if pos >= buf.len() || b == 0 {
            break;
        }
        buf[pos] = b;
        pos += 1;
    }
    if pos < buf.len() {
        buf[pos] = b':';
        pos += 1;
    }

    let gid_str = format_dec(gid);
    for &b in gid_str.iter() {
        if pos >= buf.len() || b == 0 {
            break;
        }
        buf[pos] = b;
        pos += 1;
    }
    if pos < buf.len() {
        buf[pos] = b':';
        pos += 1;
    }

    for &b in home.iter() {
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

    for &b in shell.iter() {
        if pos >= buf.len() {
            break;
        }
        buf[pos] = b;
        pos += 1;
    }
    (buf, pos)
}
