pub(super) unsafe fn format_line(
    prefix: &[u8],
    num: u64,
    suffix: &[u8],
    buf: &mut [u8],
    pos: usize,
) -> usize {
    let mut written = 0;
    if pos + written + prefix.len() <= buf.len() {
        buf[pos + written..pos + written + prefix.len()].copy_from_slice(prefix);
        written += prefix.len();
    }
    written += format_dec(num, buf, pos + written);
    if pos + written + suffix.len() <= buf.len() {
        buf[pos + written..pos + written + suffix.len()].copy_from_slice(suffix);
        written += suffix.len();
    }
    written
}

pub(super) unsafe fn format_line_raw(line: &[u8], buf: &mut [u8], pos: usize) -> usize {
    if pos + line.len() <= buf.len() {
        buf[pos..pos + line.len()].copy_from_slice(line);
        line.len()
    } else {
        0
    }
}

pub(super) unsafe fn format_dec(mut n: u64, buf: &mut [u8], pos: usize) -> usize {
    if n == 0 {
        if pos < buf.len() {
            buf[pos] = b'0';
            return 1;
        }
        return 0;
    }
    let mut tmp = [0u8; 20];
    let mut i = 20;
    while n > 0 && i > 0 {
        i -= 1;
        tmp[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    let digits = &tmp[i..];
    let n = digits.len().min(buf.len().saturating_sub(pos));
    if n > 0 {
        buf[pos..pos + n].copy_from_slice(&digits[..n]);
    }
    n
}
