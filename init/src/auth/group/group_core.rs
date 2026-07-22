#![expect(dead_code)]

use crate::auth::crypto::{copy_slice, parse_dec};
use crate::auth::GROUP_PATH;
use crate::syscalls;

#[derive(Clone, Copy)]
pub struct GroupEntry {
    pub name: [u8; 32],
    pub gid: u32,
    pub members: [u8; 256],
    pub members_len: usize,
}

pub fn parse_group(data: &[u8], groups: &mut [GroupEntry; crate::auth::MAX_GROUPS]) -> usize {
    let mut count = 0;
    let mut pos = 0;
    while pos < data.len() && count < crate::auth::MAX_GROUPS {
        let line_end = match data[pos..].iter().position(|&b| b == b'\n') {
            Some(n) => pos + n,
            None => data.len(),
        };
        let line = &data[pos..line_end];
        pos = line_end + 1;

        if line.is_empty() || line[0] == b'#' {
            continue;
        }

        let mut fields = [0usize; 4];
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
        fields[3] = start;

        if fi < 3 {
            continue;
        }

        let name = &line[fields[0]..fields[1] - 1];
        let gid_str = &line[fields[2]..fields[3] - 1];
        let members = &line[fields[3]..];

        let gid = parse_dec(gid_str);

        let mut entry = GroupEntry {
            name: [0; 32],
            gid,
            members: [0; 256],
            members_len: 0,
        };
        copy_slice(&mut entry.name, name);
        let ml = members.len().min(255);
        entry.members[..ml].copy_from_slice(&members[..ml]);
        entry.members_len = ml;
        groups[count] = entry;
        count += 1;
    }
    count
}

pub fn find_group_by_gid(
    groups: &[GroupEntry; crate::auth::MAX_GROUPS],
    count: usize,
    gid: u32,
) -> Option<usize> {
    groups[..count].iter().position(|e| e.gid == gid)
}

pub fn find_group_by_name(
    groups: &[GroupEntry; crate::auth::MAX_GROUPS],
    count: usize,
    name: &[u8],
) -> Option<usize> {
    groups[..count].iter().position(|entry| {
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

pub fn user_in_group(username: &[u8], members: &[u8]) -> bool {
    let mut pos = 0;
    while pos < members.len() {
        if members[pos] == 0 {
            break;
        }
        let end = match members[pos..].iter().position(|&b| b == b',') {
            Some(n) => pos + n,
            None => {
                let remaining = &members[pos..];
                let len = remaining
                    .iter()
                    .position(|&b| b == 0)
                    .unwrap_or(remaining.len());
                return len == username.len() && &members[pos..pos + len] == username;
            }
        };
        if end - pos == username.len() && &members[pos..end] == username {
            return true;
        }
        pos = end + 1;
    }
    false
}

pub fn read_groups(groups: &mut [GroupEntry; crate::auth::MAX_GROUPS]) -> Result<usize, i64> {
    let mut path_buf = [0u8; 64];
    let n = GROUP_PATH.len().min(63);
    path_buf[..n].copy_from_slice(&GROUP_PATH[..n]);
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

    Ok(parse_group(&buf[..total], groups))
}
