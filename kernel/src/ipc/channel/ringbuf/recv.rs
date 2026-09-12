//! Channel recv path: read bytes out of the ring buffer, wake waiting writers.

use onyx_core::errno::{Errno, KResult};
use onyx_core::ringbuf::{ring_read, ring_used};

use super::access::pid_allowed;
use super::wait::{wait_enqueue, wait_wake_all};
use crate::arch::trap_frame::TrapFrame;
use crate::ipc::channel::types::{CHAN_MAX, G_CHANNELS};

/// # Safety: `buf` must point to `len` writable bytes; `tf`, if given, must be the caller's current trap frame for sched_yield; chan_id is bounds-checked against CHAN_MAX; ring/wait-list mutation runs under the channel spinlock, always released before sched_yield.
pub unsafe fn recv(
    chan_id: u32,
    buf: *mut u8,
    len: u32,
    tf: Option<&mut TrapFrame>,
) -> KResult<u32> {
    unsafe {
        if chan_id as usize >= CHAN_MAX {
            return Err(Errno::Inval);
        }
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

        let available = ring_used(ch.head, ch.tail);
        if available == 0 {
            if let Some(tf) = tf {
                wait_enqueue(&mut ch.recv_wait);
                ch.lock.unlock();
                crate::proc::scheduler::set_need_resched(crate::proc::hart_id(), true);
                crate::proc::scheduler::sched_yield(tf);
                return Ok(0);
            }
            ch.lock.unlock();
            return Ok(0);
        }

        let dst = core::slice::from_raw_parts_mut(buf, len as usize);
        let read = ring_read(&ch.buf, &mut ch.head, ch.tail, dst);
        let wake = !ch.send_wait.is_null();
        if wake {
            wait_wake_all(&mut ch.send_wait);
        }
        ch.lock.unlock();

        if wake {
            crate::proc::scheduler::set_need_resched(crate::proc::hart_id(), true);
        }
        Ok(read)
    }
}
