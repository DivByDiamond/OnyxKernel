//! Byte ring buffer primitives shared by kernel IPC channels and PTYs.
//!
//! Counters are monotonically increasing u32 values; the wrapping difference
//! is always correct, so head/tail never need explicit modulo resets. Buffer
//! capacity is a parameter (`buf.len()` / an explicit cap) so callers with
//! fixed-size arrays and callers with slices share one tested implementation.

/// Fill level of a ring: `tail - head` in wrapping arithmetic.
#[inline]
pub fn ring_used(head: u32, tail: u32) -> u32 {
    tail.wrapping_sub(head)
}

/// Free space left in a ring of `capacity` bytes.
#[inline]
pub fn ring_free(capacity: usize, head: u32, tail: u32) -> u32 {
    capacity as u32 - ring_used(head, tail)
}

/// Write as much of `src` as fits, advancing `tail`. Returns bytes written.
pub fn ring_write(buf: &mut [u8], head: u32, tail: &mut u32, src: &[u8]) -> u32 {
    let n = (src.len() as u32).min(ring_free(buf.len(), head, *tail));
    let mut written = 0u32;
    while written < n {
        let idx = (*tail as usize) % buf.len();
        buf[idx] = src[written as usize];
        *tail = tail.wrapping_add(1);
        written += 1;
    }
    written
}

/// Read up to `dst.len()` bytes, advancing `head`. Returns bytes read.
pub fn ring_read(buf: &[u8], head: &mut u32, tail: u32, dst: &mut [u8]) -> u32 {
    let n = (dst.len() as u32).min(ring_used(*head, tail));
    let mut read = 0u32;
    while read < n {
        let idx = (*head as usize) % buf.len();
        dst[read as usize] = buf[idx];
        *head = head.wrapping_add(1);
        read += 1;
    }
    read
}

#[cfg(test)]
mod tests;
