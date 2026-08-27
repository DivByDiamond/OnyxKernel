use onyx_core::errno::{Errno, KResult};

use super::types::{CHAN_BUF_SIZE, CHAN_MAX, Channel, G_CHANNELS};
use crate::arch::trap_frame::TrapFrame;

/// Fill level of a ring: `head`/`tail` are monotonically increasing u32
/// counters, so the wrapping difference is always correct.
fn ring_used(head: u32, tail: u32) -> u32 {
    tail.wrapping_sub(head)
}

/// Free space left in a ring.
fn ring_free(head: u32, tail: u32) -> u32 {
    CHAN_BUF_SIZE as u32 - ring_used(head, tail)
}

/// Copy as many bytes of `src` as fit, advancing `tail`; returns the count.
fn ring_write(buf: &mut [u8; CHAN_BUF_SIZE], head: u32, tail: &mut u32, src: &[u8]) -> u32 {
    let n = (src.len() as u32).min(ring_free(head, *tail));
    let mut written = 0u32;
    while written < n {
        let idx = (*tail as usize) % CHAN_BUF_SIZE;
        buf[idx] = src[written as usize];
        *tail = tail.wrapping_add(1);
        written += 1;
    }
    written
}

/// Read up to `dst.len()` bytes, advancing `head`; returns the count.
fn ring_read(buf: &[u8; CHAN_BUF_SIZE], head: &mut u32, tail: u32, dst: &mut [u8]) -> u32 {
    let n = (dst.len() as u32).min(ring_used(*head, tail));
    let mut read = 0u32;
    while read < n {
        let idx = (*head as usize) % CHAN_BUF_SIZE;
        dst[read as usize] = buf[idx];
        *head = head.wrapping_add(1);
        read += 1;
    }
    read
}

fn pid_allowed(ch: &Channel, pid: u32) -> bool {
    if pid == ch.owner_pid {
        return true;
    }
    for &c in ch.clients[..ch.num_clients as usize].iter() {
        if c == pid {
            return true;
        }
    }
    false
}

unsafe fn wait_enqueue(wait_head: &mut *mut crate::proc::Proc) {
    unsafe {
        let p = crate::proc::current() as *mut crate::proc::Proc;
        (*p).state = crate::proc::ProcState::Waiting;
        (*p).wait_next = *wait_head;
        *wait_head = p;
    }
}

unsafe fn wait_wake_all(wait_head: &mut *mut crate::proc::Proc) {
    unsafe {
        let mut cur = *wait_head;
        while !cur.is_null() {
            let next = (*cur).wait_next;
            (*cur).state = crate::proc::ProcState::Ready;
            (*cur).wait_next = core::ptr::null_mut();
            cur = next;
        }
        *wait_head = core::ptr::null_mut();
    }
}

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
        // B5 fix: all state checks + ring mutation + sleep/wake decision happen
        // under the per-channel spinlock. The lock is ALWAYS released before
        // sched_yield — never hold a SpinLock across a context switch.
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

        let available = ring_free(ch.head, ch.tail);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ring_fill_to_capacity_then_drain() {
        let mut buf = [0u8; CHAN_BUF_SIZE];
        let (mut head, mut tail) = (0u32, 0u32);
        let n = CHAN_BUF_SIZE as u32;
        let src: alloc::vec::Vec<u8> = (0..n).map(|i| i as u8).collect();
        assert_eq!(ring_write(&mut buf, head, &mut tail, &src), n);
        assert_eq!(ring_free(head, tail), 0);
        assert_eq!(ring_write(&mut buf, head, &mut tail, &[0xAA]), 0);
        let mut dst = alloc::vec![0u8; CHAN_BUF_SIZE];
        assert_eq!(ring_read(&buf, &mut head, tail, &mut dst), n);
        assert_eq!(dst, src);
    }

    #[test]
    fn test_ring_wraparound_and_partial_io() {
        let mut buf = [0u8; CHAN_BUF_SIZE];
        let (mut head, mut tail) = (0u32, 0u32);
        // Push past the physical end of the buffer: indices must wrap.
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
    fn test_pid_allowed_owner_client_stranger() {
        let mut ch = Channel::zeroed();
        ch.owner_pid = 7;
        ch.clients[0] = 9;
        ch.num_clients = 1;
        assert!(pid_allowed(&ch, 7));
        assert!(pid_allowed(&ch, 9));
        assert!(!pid_allowed(&ch, 8));
    }

    #[test]
    fn test_send_recv_error_paths() {
        unsafe {
            let one = [0x42u8];
            let n = CHAN_BUF_SIZE as u32;
            let bad = CHAN_MAX as u32;
            let mut drain = alloc::vec![0u8; CHAN_BUF_SIZE];
            assert_eq!(send(bad, one.as_ptr(), 1, None), Err(Errno::Inval));
            assert_eq!(recv(bad, drain.as_mut_ptr(), 1, None), Err(Errno::Inval));
            assert_eq!(send(0, one.as_ptr(), 1, None), Err(Errno::Pipe));
            assert_eq!(recv(0, drain.as_mut_ptr(), 1, None), Err(Errno::Pipe));
            // Live channel owned by pid 0 (host current_pid()==0).
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
