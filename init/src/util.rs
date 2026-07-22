pub(crate) fn cstr_len(buf: &[u8]) -> usize {
    buf.iter().position(|&b| b == 0).unwrap_or(buf.len())
}

pub(crate) fn is_dot_or_dotdot(name: &[u8]) -> bool {
    if name.len() == 1 && name[0] == b'.' {
        return true;
    }
    if name.len() == 2 && name[0] == b'.' && name[1] == b'.' {
        return true;
    }
    false
}

pub(crate) fn split_first_token(s: &[u8]) -> (&[u8], &[u8]) {
    let mut i = 0;
    while i < s.len() && s[i] != b' ' && s[i] != b'\t' {
        i += 1;
    }
    (&s[..i], &s[i..])
}

pub(crate) fn trim_space(s: &[u8]) -> &[u8] {
    let mut i = 0;
    while i < s.len() && (s[i] == b' ' || s[i] == b'\t') {
        i += 1;
    }
    &s[i..]
}

pub(crate) fn write_response(resp: &mut [u8], s: &[u8]) -> usize {
    let n = s.len().min(resp.len());
    resp[..n].copy_from_slice(s);
    n
}

pub(crate) fn format_dec(n: i64) -> [u8; 12] {
    let mut buf = [0u8; 12];
    let mut pos = 11;
    if n == 0 {
        buf[10] = b'0';
        return buf;
    }
    let neg = n < 0;
    let mut val = if neg { (-(n as i128)) as u64 } else { n as u64 };
    while val > 0 && pos > 0 {
        pos -= 1;
        buf[pos] = b'0' + (val % 10) as u8;
        val /= 10;
    }
    if neg && pos > 0 {
        pos -= 1;
        buf[pos] = b'-';
    }
    buf
}

pub(crate) fn dec_slice(s: &[u8; 12]) -> &[u8] {
    let start = s.iter().position(|&b| b != 0).unwrap_or(s.len());
    let end = s.iter().rposition(|&b| b != 0).map(|i| i + 1).unwrap_or(0);
    if start < end {
        &s[start..end]
    } else {
        &[]
    }
}

pub(crate) unsafe fn write_dec(n: i64) {
    let s = format_dec(n);
    let slice = dec_slice(&s);
    if !slice.is_empty() {
        crate::syscalls::write(1, slice.as_ptr(), slice.len());
    }
}
