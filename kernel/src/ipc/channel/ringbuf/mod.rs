//! IPC channel ring buffer: send/recv over a lock-protected ring, with
//! sleep/wake lists for blocking waiters. Split by responsibility:
//! `access` (pid permission checks), `wait` (sleep/wake list management),
//! `send`/`recv` (the two data-path operations).

mod access;
mod recv;
mod send;
mod wait;

pub use onyx_core::ringbuf::{ring_free, ring_read, ring_used, ring_write};
pub use recv::recv;
pub use send::send;
pub use wait::disconnect_waiter;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::channel::types::{CHAN_BUF_SIZE, CHAN_MAX, Channel, G_CHANNELS};
    use onyx_core::errno::Errno;

    #[test]
    fn test_ring_fill_to_capacity_then_drain() {
        let mut buf = [0u8; CHAN_BUF_SIZE];
        let (mut head, mut tail) = (0u32, 0u32);
        let n = CHAN_BUF_SIZE as u32;
        let src: alloc::vec::Vec<u8> = (0..n).map(|i| i as u8).collect();
        assert_eq!(ring_write(&mut buf, head, &mut tail, &src), n);
        assert_eq!(ring_free(CHAN_BUF_SIZE, head, tail), 0);
        assert_eq!(ring_write(&mut buf, head, &mut tail, &[0xAA]), 0);
        let mut dst = alloc::vec![0u8; CHAN_BUF_SIZE];
        assert_eq!(ring_read(&buf, &mut head, tail, &mut dst), n);
        assert_eq!(dst, src);
    }

    #[test]
    fn test_ring_wraparound_and_partial_io() {
        let mut buf = [0u8; CHAN_BUF_SIZE];
        let (mut head, mut tail) = (0u32, 0u32);
        // Indices must wrap past the physical end of the buffer.
        let n = CHAN_BUF_SIZE as u32;
        let src: alloc::vec::Vec<u8> = (0..5000u32).map(|i| (i * 7) as u8).collect();
        assert_eq!(ring_write(&mut buf, head, &mut tail, &src), n);
        let mut dst = [0u8; 100];
        assert_eq!(ring_read(&buf, &mut head, tail, &mut dst), 100);
        assert_eq!(&dst[..], &src[0..100]);
        assert_eq!(ring_write(&mut buf, head, &mut tail, &src[..100]), 100);
        let (mut head, mut tail) = (u32::MAX - 2, u32::MAX - 2);
        assert_eq!(ring_write(&mut buf, head, &mut tail, &[1, 2, 3, 4, 5]), 5);
        let mut out = [0u8; 5];
        assert_eq!(ring_read(&buf, &mut head, tail, &mut out), 5);
        assert_eq!(out, [1, 2, 3, 4, 5]);
    }

    #[test]
    fn test_send_recv_error_paths() {
        // SAFETY: single-threaded host test; bad ids error before any G_CHANNELS deref, chan 0 re-zeroed after.
        unsafe {
            let one = [0x42u8];
            let n = CHAN_BUF_SIZE as u32;
            let bad = CHAN_MAX as u32;
            let mut drain = alloc::vec![0u8; CHAN_BUF_SIZE];
            assert_eq!(send(bad, one.as_ptr(), 1, None), Err(Errno::Inval));
            assert_eq!(recv(bad, drain.as_mut_ptr(), 1, None), Err(Errno::Inval));
            assert_eq!(send(0, one.as_ptr(), 1, None), Err(Errno::Pipe));
            assert_eq!(recv(0, drain.as_mut_ptr(), 1, None), Err(Errno::Pipe));
            G_CHANNELS[0] = Channel::zeroed();
            G_CHANNELS[0].used = true;
            let blob = alloc::vec![0x11u8; CHAN_BUF_SIZE];
            assert_eq!(send(0, blob.as_ptr(), n, None), Ok(n));
            assert_eq!(send(0, one.as_ptr(), 1, None), Err(Errno::Busy));
            assert_eq!(recv(0, drain.as_mut_ptr(), n, None), Ok(n));
            assert_eq!(recv(0, drain.as_mut_ptr(), 1, None), Ok(0));
            G_CHANNELS[0] = Channel::zeroed();
        }
    }
}
