#![allow(dead_code)]
use crate::syscalls;

pub const PASSWD_PATH: &[u8] = b"/etc/passwd";
pub const SHADOW_PATH: &[u8] = b"/etc/shadow";
pub const MAX_USERS: usize = 16;
pub const MAX_LINE: usize = 256;

// ============================================================================
// SHA-256 implementation (no_std, pure Rust)
// ============================================================================

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

fn rotr(x: u32, n: u32) -> u32 {
    (x >> n) | (x << (32u32.wrapping_sub(n)))
}

fn ch(x: u32, y: u32, z: u32) -> u32 {
    (x & y) ^ ((!x) & z)
}

fn maj(x: u32, y: u32, z: u32) -> u32 {
    (x & y) ^ (x & z) ^ (y & z)
}

fn SIG0(x: u32) -> u32 {
    rotr(x, 2) ^ rotr(x, 13) ^ rotr(x, 22)
}

fn SIG1(x: u32) -> u32 {
    rotr(x, 6) ^ rotr(x, 11) ^ rotr(x, 25)
}

fn sigma0(x: u32) -> u32 {
    rotr(x, 7) ^ rotr(x, 18) ^ (x >> 3)
}

fn sigma1(x: u32) -> u32 {
    rotr(x, 17) ^ rotr(x, 19) ^ (x >> 10)
}

pub struct Sha256State {
    state: [u32; 8],
    buf: [u8; 64],
    buf_len: usize,
    total_len: u64,
}

fn sha256_compress(state: &mut [u32; 8], block: &[u8; 64]) {
    let mut w = [0u32; 64];
    for t in 0..16 {
        w[t] = u32::from_be_bytes([
            block[t * 4],
            block[t * 4 + 1],
            block[t * 4 + 2],
            block[t * 4 + 3],
        ]);
    }
    for t in 16..64 {
        let s0 = sigma0(w[t - 15]);
        let s1 = sigma1(w[t - 2]);
        w[t] = s1
            .wrapping_add(w[t - 7])
            .wrapping_add(s0)
            .wrapping_add(w[t - 16]);
    }

    let mut a = state[0];
    let mut b = state[1];
    let mut c = state[2];
    let mut d = state[3];
    let mut e = state[4];
    let mut f = state[5];
    let mut g = state[6];
    let mut h = state[7];

    for t in 0..64 {
        let t1 = h
            .wrapping_add(SIG1(e))
            .wrapping_add(ch(e, f, g))
            .wrapping_add(K[t])
            .wrapping_add(w[t]);
        let t2 = SIG0(a).wrapping_add(maj(a, b, c));
        h = g;
        g = f;
        f = e;
        e = d.wrapping_add(t1);
        d = c;
        c = b;
        b = a;
        a = t1.wrapping_add(t2);
    }

    state[0] = state[0].wrapping_add(a);
    state[1] = state[1].wrapping_add(b);
    state[2] = state[2].wrapping_add(c);
    state[3] = state[3].wrapping_add(d);
    state[4] = state[4].wrapping_add(e);
    state[5] = state[5].wrapping_add(f);
    state[6] = state[6].wrapping_add(g);
    state[7] = state[7].wrapping_add(h);
}

pub fn sha256_init() -> Sha256State {
    Sha256State {
        state: [
            0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
            0x5be0cd19,
        ],
        buf: [0u8; 64],
        buf_len: 0,
        total_len: 0,
    }
}

pub fn sha256_update(state: &mut Sha256State, data: &[u8]) {
    for &b in data {
        state.buf[state.buf_len] = b;
        state.buf_len += 1;
        state.total_len = state.total_len.wrapping_add(1);
        if state.buf_len == 64 {
            sha256_compress(&mut state.state, &state.buf);
            state.buf_len = 0;
        }
    }
}

pub fn sha256_final(state: Sha256State) -> [u8; 32] {
    let mut state = state;
    let bit_len = state.total_len.wrapping_mul(8);

    state.buf[state.buf_len] = 0x80;
    state.buf_len += 1;

    while state.buf_len % 64 != 56 {
        if state.buf_len == 64 {
            sha256_compress(&mut state.state, &state.buf);
            state.buf_len = 0;
        }
        state.buf[state.buf_len] = 0;
        state.buf_len += 1;
    }

    state.buf[56..64].copy_from_slice(&bit_len.to_be_bytes());
    sha256_compress(&mut state.state, &state.buf);

    let mut out = [0u8; 32];
    for i in 0..8 {
        out[i * 4..(i + 1) * 4].copy_from_slice(&state.state[i].to_be_bytes());
    }
    out
}

pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut state = sha256_init();
    sha256_update(&mut state, data);
    sha256_final(state)
}

// ============================================================================
// Hex encoding/decoding
// ============================================================================

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

fn hex_decode_8(hex: &[u8]) -> [u8; 8] {
    let mut out = [0u8; 8];
    let n = (hex.len() / 2).min(8);
    for i in 0..n {
        out[i] = (hex_val(hex[i * 2]) << 4) | hex_val(hex[i * 2 + 1]);
    }
    out
}

// ============================================================================
// Constant-time comparison
// ============================================================================

pub fn const_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut r = 0u8;
    for i in 0..a.len() {
        r |= a[i] ^ b[i];
    }
    r == 0
}

// ============================================================================
// Salt generation (LCG PRNG seeded with PID)
// ============================================================================

fn generate_salt() -> [u8; 8] {
    let pid = unsafe { syscalls::getpid() } as u64;
    let mut seed = pid.wrapping_mul(1103515245).wrapping_add(12345);
    let mut salt = [0u8; 8];
    for i in 0..8 {
        seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
        salt[i] = (seed >> 16) as u8;
    }
    salt
}

// ============================================================================
// Password file types and helpers
// ============================================================================

#[derive(Clone, Copy)]
pub struct PasswdEntry {
    pub name: [u8; 32],
    pub uid: u32,
    pub gid: u32,
    pub home: [u8; 64],
    pub shell: [u8; 32],
}

pub fn parse_passwd(data: &[u8], users: &mut [PasswdEntry; MAX_USERS]) -> usize {
    let mut count = 0;
    let mut pos = 0;
    while pos < data.len() && count < MAX_USERS {
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

pub fn find_user(users: &[PasswdEntry; MAX_USERS], count: usize, name: &[u8]) -> Option<usize> {
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

pub fn find_user_by_uid(users: &[PasswdEntry; MAX_USERS], count: usize, uid: u32) -> Option<usize> {
    users[..count].iter().position(|e| e.uid == uid)
}

pub fn read_passwd(users: &mut [PasswdEntry; MAX_USERS]) -> Result<usize, i64> {
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

// ============================================================================
// Shadow password operations (SHA-256 + salt)
// ============================================================================

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

    let mut combined = [0u8; 72];
    let mut combined_len = 0;
    for &b in password {
        if combined_len >= combined.len() {
            break;
        }
        combined[combined_len] = b;
        combined_len += 1;
    }
    for &b in &salt_bytes {
        if combined_len >= combined.len() {
            break;
        }
        combined[combined_len] = b;
        combined_len += 1;
    }

    let computed_hash = sha256(&combined[..combined_len]);
    let computed_hex = bytes_to_hex(&computed_hash);

    const_time_eq(&computed_hex[..64], stored_hash_hex)
}

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
            let entry = format_shadow_entry(username, new_password);
            let copy_end = (out_pos + entry.len()).min(out.len());
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
        let entry = format_shadow_entry(username, new_password);
        let copy_end = (out_pos + entry.len()).min(out.len());
        let to_copy = copy_end - out_pos;
        out[out_pos..copy_end].copy_from_slice(&entry[..to_copy]);
        out_pos = copy_end;
        if out_pos < out.len() {
            out[out_pos] = b'\n';
            out_pos += 1;
        }
    }

    let fd = unsafe { syscalls::create(path_buf.as_ptr(), 0o600, 0) };
    if fd < 0 {
        return Err(fd);
    }
    let _ = unsafe { syscalls::write_fd(fd as u64, out.as_ptr(), out_pos) };
    unsafe { syscalls::close(fd as u64) };
    Ok(())
}

fn format_shadow_entry(username: &[u8], password: &[u8]) -> [u8; 128] {
    let salt = generate_salt();

    let mut combined = [0u8; 72];
    let mut combined_len = 0;
    for &b in password {
        if combined_len >= combined.len() {
            break;
        }
        combined[combined_len] = b;
        combined_len += 1;
    }
    for &b in &salt {
        if combined_len >= combined.len() {
            break;
        }
        combined[combined_len] = b;
        combined_len += 1;
    }

    let hash = sha256(&combined[..combined_len]);
    let salt_hex = {
        let h = bytes_to_hex(&salt);
        let mut s = [0u8; 16];
        s.copy_from_slice(&h[..16]);
        s
    };
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
    if pos < buf.len() {
        buf[pos] = b'$';
        pos += 1;
    }
    if pos < buf.len() {
        buf[pos] = b'5';
        pos += 1;
    }
    if pos < buf.len() {
        buf[pos] = b'$';
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
    buf
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
            let entry = format_passwd_entry(username, uid, gid, home, shell);
            let copy_end = (out_pos + entry.len()).min(out.len());
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
        let entry = format_passwd_entry(username, uid, gid, home, shell);
        let copy_end = (out_pos + entry.len()).min(out.len());
        let to_copy = copy_end - out_pos;
        out[out_pos..copy_end].copy_from_slice(&entry[..to_copy]);
        out_pos = copy_end;
        if out_pos < out.len() {
            out[out_pos] = b'\n';
            out_pos += 1;
        }
    }

    let fd = unsafe { syscalls::create(path_buf.as_ptr(), 0o644, 0) };
    if fd < 0 {
        return Err(fd);
    }
    let _ = unsafe { syscalls::write_fd(fd as u64, out.as_ptr(), out_pos) };
    unsafe { syscalls::close(fd as u64) };
    Ok(())
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

    let fd = unsafe { syscalls::create(path_buf.as_ptr(), 0o644, 0) };
    if fd < 0 {
        return Err(fd);
    }
    let _ = unsafe { syscalls::write_fd(fd as u64, out.as_ptr(), out_pos) };
    unsafe { syscalls::close(fd as u64) };
    Ok(())
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

    let fd = unsafe { syscalls::create(path_buf.as_ptr(), 0o600, 0) };
    if fd < 0 {
        return Err(fd);
    }
    let _ = unsafe { syscalls::write_fd(fd as u64, out.as_ptr(), out_pos) };
    unsafe { syscalls::close(fd as u64) };
    Ok(())
}

fn format_passwd_entry(
    username: &[u8],
    uid: u32,
    gid: u32,
    home: &[u8],
    shell: &[u8],
) -> [u8; 256] {
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
        if b == 0 {
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
    buf
}

fn format_dec(n: u32) -> [u8; 12] {
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

fn parse_dec(s: &[u8]) -> u32 {
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

fn copy_slice(dst: &mut [u8], src: &[u8]) {
    let n = dst.len().min(src.len());
    dst[..n].copy_from_slice(&src[..n]);
}
