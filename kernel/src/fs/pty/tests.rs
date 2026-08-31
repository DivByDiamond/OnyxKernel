//! Host tests for the PTY core: pair lifetime, both directions, ring-full
//! and EPIPE semantics. Blocking (yield-loop) behavior lives in stream.rs
//! and needs a trap frame, so only the non-blocking layer is covered here.

use super::{G_PTYS, PTY_BUF_CAP, PTY_MAX, Pty, PtyRing, PtyWinsize};
use crate::fs::pty;
use core::sync::atomic::{AtomicBool, Ordering};
use onyx_core::errno::Errno;

/// Serializes tests that share the global G_PTYS table (host test harness
/// runs them in parallel threads). Panic-safe via the Drop guard.
static TEST_LOCK: AtomicBool = AtomicBool::new(false);

struct TestGuard;

impl TestGuard {
    fn new() -> Self {
        while TEST_LOCK
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }
        TestGuard
    }
}

impl Drop for TestGuard {
    fn drop(&mut self) {
        TEST_LOCK.store(false, Ordering::Release);
    }
}

/// Reset every pair after a test so tests stay order-independent.
fn reset_all() {
    // SAFETY: single-threaded host test; no other hart touches G_PTYS.
    unsafe {
        let base = &raw mut G_PTYS;
        for slot in (*base).iter_mut() {
            *slot = Pty::new();
        }
    }
}

#[test]
fn test_alloc_and_free_roundtrip() {
    reset_all();
    let _g = TestGuard::new();
    let a = pty::alloc().expect("alloc on fresh table");
    // SAFETY: host test context, idx from alloc above.
    unsafe {
        assert!(pty::is_used(a));
        assert!(!pty::is_used((a + 1) as u32));
    }
    pty::free(a);
    // SAFETY: host test context.
    unsafe {
        assert!(!pty::is_used(a));
    }
}

#[test]
fn test_alloc_exhaustion_returns_again() {
    reset_all();
    let _g = TestGuard::new();
    let mut ids = alloc::vec::Vec::new();
    for _ in 0..PTY_MAX {
        ids.push(pty::alloc().expect("alloc until exhaustion"));
    }
    assert_eq!(pty::alloc(), Err(Errno::Again));
    // Freeing one slot makes it claimable again.
    pty::free(ids[0]);
    assert!(pty::alloc().is_ok());
}

#[test]
fn test_master_to_slave_roundtrip() {
    reset_all();
    let _g = TestGuard::new();
    let idx = pty::alloc().expect("alloc");
    // SAFETY: host test, valid idx, kernel-side buffers below.
    unsafe {
        assert_eq!(pty::side_read(idx, false, [0u8; 4].as_mut_ptr(), 4), Ok(0));
        let msg = b"hello";
        assert_eq!(pty::side_write(idx, true, msg.as_ptr(), 5), Ok(5));
        let mut dst = [0u8; 8];
        assert_eq!(pty::side_read(idx, false, dst.as_mut_ptr(), 8), Ok(5));
        assert_eq!(&dst[..5], msg);
    }
    pty::free(idx);
}

#[test]
fn test_slave_to_master_roundtrip() {
    reset_all();
    let _g = TestGuard::new();
    let idx = pty::alloc().expect("alloc");
    // SAFETY: host test, valid idx, kernel-side buffers below.
    unsafe {
        let msg = b"exit\n";
        assert_eq!(pty::side_write(idx, false, msg.as_ptr(), 5), Ok(5));
        let mut dst = [0u8; 8];
        assert_eq!(pty::side_read(idx, true, dst.as_mut_ptr(), 8), Ok(5));
        assert_eq!(&dst[..5], msg);
    }
    pty::free(idx);
}

#[test]
fn test_partial_write_when_full() {
    reset_all();
    let _g = TestGuard::new();
    let idx = pty::alloc().expect("alloc");
    // SAFETY: host test, valid idx, kernel-side buffers below.
    unsafe {
        let blob = [0x5Au8; PTY_BUF_CAP + 16];
        // First push fills the m2s ring exactly.
        assert_eq!(
            pty::side_write(idx, true, blob.as_ptr(), PTY_BUF_CAP as u32),
            Ok(PTY_BUF_CAP as u32)
        );
        // Ring is full: a non-blocking write stores nothing.
        assert_eq!(pty::side_write(idx, true, blob.as_ptr(), 16), Ok(0));
        // Drain 8 bytes; exactly 8 more now fit.
        let mut dst = [0u8; 8];
        assert_eq!(pty::side_read(idx, false, dst.as_mut_ptr(), 8), Ok(8));
        assert_eq!(pty::side_write(idx, true, blob.as_ptr(), 16), Ok(8));
    }
    pty::free(idx);
}

#[test]
fn test_pipe_after_master_free() {
    reset_all();
    let _g = TestGuard::new();
    let idx = pty::alloc().expect("alloc");
    // SAFETY: host test, valid idx, kernel-side buffers below.
    unsafe {
        pty::free(idx);
        let msg = b"x";
        assert_eq!(
            pty::side_write(idx, false, msg.as_ptr(), 1),
            Err(Errno::Pipe)
        );
        let mut dst = [0u8; 1];
        assert_eq!(
            pty::side_read(idx, true, dst.as_mut_ptr(), 1),
            Err(Errno::Pipe)
        );
        assert_eq!(
            pty::side_read(idx, false, dst.as_mut_ptr(), 1),
            Err(Errno::Pipe)
        );
    }
}

#[test]
fn test_poll_readiness_transitions() {
    reset_all();
    let _g = TestGuard::new();
    let idx = pty::alloc().expect("alloc");
    // SAFETY: host test, valid idx.
    unsafe {
        // Idle pair: master has nothing to read; both sides writable.
        assert_eq!(pty::side_poll(idx, true), (false, true));
        assert_eq!(pty::side_poll(idx, false), (false, true));
        let msg = b"key";
        assert_eq!(pty::side_write(idx, false, msg.as_ptr(), 3), Ok(3));
        assert_eq!(pty::side_poll(idx, true), (true, true));
        let mut dst = [0u8; 8];
        assert_eq!(pty::side_read(idx, true, dst.as_mut_ptr(), 8), Ok(3));
        assert_eq!(pty::side_poll(idx, true), (false, true));
    }
    pty::free(idx);
}

#[test]
fn test_winsize_default_and_set() {
    reset_all();
    let _g = TestGuard::new();
    let idx = pty::alloc().expect("alloc");
    // SAFETY: host test, valid idx.
    unsafe {
        let ws = pty::winsize(idx);
        assert_eq!((ws.rows, ws.cols), (24, 80));
        pty::set_winsize(
            idx,
            PtyWinsize {
                rows: 40,
                cols: 120,
                xpixel: 960,
                ypixel: 640,
            },
        );
        let ws = pty::winsize(idx);
        assert_eq!(
            (ws.rows, ws.cols, ws.xpixel, ws.ypixel),
            (40, 120, 960, 640)
        );
    }
    pty::free(idx);
}

#[test]
fn test_pyring_partial_then_rest() {
    let mut r = PtyRing::new();
    assert_eq!(r.used(), 0);
    let msg = b"abcdef";
    assert_eq!(r.push(msg), 6);
    assert_eq!(r.used(), 6);
    let mut dst = [0u8; 2];
    assert_eq!(r.pop(&mut dst), 2);
    assert_eq!(&dst, b"ab");
    let mut rest = [0u8; 16];
    assert_eq!(r.pop(&mut rest), 4);
    assert_eq!(&rest[..4], b"cdef");
    assert_eq!(r.used(), 0);
}
