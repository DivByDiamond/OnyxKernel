//! One PTY pipeline direction: a byte ring with wrapping counters, backed
//! by the shared `onyx_core::ringbuf` primitives (same implementation the
//! IPC channels use).

use super::PTY_BUF_CAP;
use onyx_core::ringbuf::{ring_read, ring_used, ring_write};

pub struct PtyRing {
    pub buf: [u8; PTY_BUF_CAP],
    pub head: u32,
    pub tail: u32,
}

impl PtyRing {
    pub const fn new() -> Self {
        PtyRing {
            buf: [0; PTY_BUF_CAP],
            head: 0,
            tail: 0,
        }
    }
    /// Bytes pending on this side (readable right now).
    pub fn used(&self) -> u32 {
        ring_used(self.head, self.tail)
    }

    /// Copy `dst.len()` bytes out, advancing head. Returns bytes read.
    pub fn pop(&mut self, dst: &mut [u8]) -> u32 {
        ring_read(&self.buf, &mut self.head, self.tail, dst)
    }

    /// Copy as much of `src` in as fits, advancing tail. Returns written.
    pub fn push(&mut self, src: &[u8]) -> u32 {
        ring_write(&mut self.buf, self.head, &mut self.tail, src)
    }
}

impl Default for PtyRing {
    fn default() -> Self {
        Self::new()
    }
}
