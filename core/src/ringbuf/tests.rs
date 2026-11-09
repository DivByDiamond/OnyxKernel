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
