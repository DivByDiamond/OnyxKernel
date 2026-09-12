//! Channel send path: write bytes into the ring buffer, wake waiting readers.

use onyx_core::errno::{Errno, KResult};
use onyx_core::ringbuf::{ring_free, ring_write};

use super::access::pid_allowed;
use super::wait::{wait_enqueue, wait_wake_all};
use crate::arch::trap_frame::TrapFrame;
use crate::ipc::channel::types::{CHAN_BUF_SIZE, CHAN_MAX, G_CHANNELS};

/// # Safety: `buf` must point to `len` readable bytes; `tf`, if given, must be the caller's current trap frame for sched_yield; chan_id is bounds-checked against CHAN_MAX; ring/wait-list mutation runs under the channel spinlock, always released before sched_yield.
pub unsafe fn send(
    chan_id: u32,
    buf: *const u8,
    len: u32,
    tf: Option<&mut TrapFrame>,
) -> KResult<u32> {
    unsafe {
        if chan_id as usize >= CHAN_MAX {
            return Err(Errno::Inval);
        }
        // B5 fix: state checks + ring mutation + wake decision under the per-channel spinlock; lock ALWAYS released before sched_yield.
        let ch = &mut G_CHANNELS[chan_id as usize];
        ch.lock.lock();
        if !ch.used || ch.closed {
            ch.lock.unlock();
            return Err(Errno::Pipe);
        }
        let cur_pid = crate::proc::current_pid();
        if !pid_allowed(ch, cur_pid) {
            ch.lock.unlock();
            return Err(Errno::Perm);
        }

        let available = ring_free(CHAN_BUF_SIZE, ch.head, ch.tail);
        if available < len {
            if let Some(tf) = tf {
                wait_enqueue(&mut ch.send_wait);
                ch.lock.unlock();
                crate::proc::scheduler::set_need_resched(crate::proc::hart_id(), true);
                crate::proc::scheduler::sched_yield(tf);
                return Err(Errno::Busy);
            }
            ch.lock.unlock();
            return Err(Errno::Busy);
        }

        let src = core::slice::from_raw_parts(buf, len as usize);
        let written = ring_write(&mut ch.buf, ch.head, &mut ch.tail, src);
        let wake = !ch.recv_wait.is_null();
        if wake {
            wait_wake_all(&mut ch.recv_wait);
        }
        ch.lock.unlock();

        if wake {
            crate::proc::scheduler::set_need_resched(crate::proc::hart_id(), true);
        }
        Ok(written)
    }
}
