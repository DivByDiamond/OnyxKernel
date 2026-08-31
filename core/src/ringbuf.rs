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
mod tests {
    use super::*;

    #[test]
    fn test_fill_to_capacity_then_drain() {
        let mut buf = [0u8; 64];
        let (mut head, mut tail) = (0u32, 0u32);
        let src: alloc::vec::Vec<u8> = (0..64u32).map(|i| i as u8).collect();
        assert_eq!(ring_write(&mut buf, head, &mut tail, &src), 64);
        assert_eq!(ring_free(64, head, tail), 0);
        assert_eq!(ring_write(&mut buf, head, &mut tail, &[0xAA]), 0);
        let mut dst = alloc::vec![0u8; 64];
        assert_eq!(ring_read(&buf, &mut head, tail, &mut dst), 64);
        assert_eq!(dst, src);
        assert_eq!(ring_used(head, tail), 0);
    }

    #[test]
    fn test_wraparound_and_partial_io() {
        let mut buf = [0u8; 64];
        let (mut head, mut tail) = (0u32, 0u32);
        // Wrap the indices past the physical end of the buffer.
        let src: alloc::vec::Vec<u8> = (0..5000u32).map(|i| (i * 7) as u8).collect();
        assert_eq!(ring_write(&mut buf, head, &mut tail, &src), 64);
        let mut dst = [0u8; 10];
        assert_eq!(ring_read(&buf, &mut head, tail, &mut dst), 10);
        assert_eq!(&dst[..], &src[0..10]);
        assert_eq!(ring_write(&mut buf, head, &mut tail, &src[..10]), 10);
        assert_eq!(ring_used(head, tail), 64);
    }

    #[test]
    fn test_counter_wrapping_near_u32_max() {
        let mut buf = [0u8; 8];
        let (mut head, mut tail) = (u32::MAX - 2, u32::MAX - 2);
        assert_eq!(ring_write(&mut buf, head, &mut tail, &[1, 2, 3, 4, 5]), 5);
        let mut out = [0u8; 5];
        assert_eq!(ring_read(&buf, &mut head, tail, &mut out), 5);
        assert_eq!(out, [1, 2, 3, 4, 5]);
    }

    #[test]
    fn test_oversized_dst_reads_only_available() {
        let mut buf = [0u8; 4];
        let (mut head, mut tail) = (0u32, 0u32);
        assert_eq!(ring_write(&mut buf, head, &mut tail, b"abc"), 3);
        let mut dst = [0u8; 16];
        assert_eq!(ring_read(&buf, &mut head, tail, &mut dst), 3);
        assert_eq!(&dst[..3], b"abc");
    }
}
