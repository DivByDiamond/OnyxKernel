use crate::arch::trap_frame::TrapFrame;
use onyx_core::errno::{Errno, KResult};

use super::types::{CHAN_BUF_SIZE, CHAN_MAX, Channel, G_CHANNELS};

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
    let p = crate::proc::current() as *mut crate::proc::Proc;
    (*p).state = crate::proc::ProcState::Waiting;
    (*p).wait_next = *wait_head;
    *wait_head = p;
}

unsafe fn wait_wake_all(wait_head: &mut *mut crate::proc::Proc) {
    let mut cur = *wait_head;
    while !cur.is_null() {
        let next = (*cur).wait_next;
        (*cur).state = crate::proc::ProcState::Ready;
        (*cur).wait_next = core::ptr::null_mut();
        cur = next;
    }
    *wait_head = core::ptr::null_mut();
}

pub unsafe fn send(
    chan_id: u32,
    buf: *const u8,
    len: u32,
    tf: Option<&mut TrapFrame>,
) -> KResult<u32> {
    if chan_id as usize >= CHAN_MAX {
        return Err(Errno::Inval);
    }
    let ch = &mut G_CHANNELS[chan_id as usize];
    if !ch.used || ch.closed {
        return Err(Errno::Pipe);
    }
    let cur_pid = crate::proc::current_pid();
    if !pid_allowed(ch, cur_pid) {
        return Err(Errno::Perm);
    }

    let available = CHAN_BUF_SIZE as u32 - ch.tail.wrapping_sub(ch.head);
    if available < len {
        if let Some(tf) = tf {
            wait_enqueue(&mut ch.send_wait);
            crate::proc::scheduler::set_need_resched(crate::proc::hart_id(), true);
            crate::proc::scheduler::sched_yield(tf);
            return Err(Errno::Busy);
        }
        return Err(Errno::Busy);
    }

    let mut written = 0u32;
    while written < len {
        let idx = (ch.tail as usize) % CHAN_BUF_SIZE;
        ch.buf[idx] = *buf.add(written as usize);
        ch.tail = ch.tail.wrapping_add(1);
        written += 1;
    }

    if !ch.recv_wait.is_null() {
        wait_wake_all(&mut ch.recv_wait);
        crate::proc::scheduler::set_need_resched(crate::proc::hart_id(), true);
    }
    Ok(written)
}

pub unsafe fn recv(
    chan_id: u32,
    buf: *mut u8,
    len: u32,
    tf: Option<&mut TrapFrame>,
) -> KResult<u32> {
    if chan_id as usize >= CHAN_MAX {
        return Err(Errno::Inval);
    }
    let ch = &mut G_CHANNELS[chan_id as usize];
    if !ch.used || ch.closed {
        return Err(Errno::Pipe);
    }
    let cur_pid = crate::proc::current_pid();
    if !pid_allowed(ch, cur_pid) {
        return Err(Errno::Perm);
    }

    let available = ch.tail.wrapping_sub(ch.head);
    if available == 0 {
        if let Some(tf) = tf {
            wait_enqueue(&mut ch.recv_wait);
            crate::proc::scheduler::set_need_resched(crate::proc::hart_id(), true);
            crate::proc::scheduler::sched_yield(tf);
            return Ok(0);
        }
        return Ok(0);
    }
    let to_read = len.min(available);

    let mut read = 0u32;
    while read < to_read {
        let idx = (ch.head as usize) % CHAN_BUF_SIZE;
        *buf.add(read as usize) = ch.buf[idx];
        ch.head = ch.head.wrapping_add(1);
        read += 1;
    }

    if !ch.send_wait.is_null() {
        wait_wake_all(&mut ch.send_wait);
        crate::proc::scheduler::set_need_resched(crate::proc::hart_id(), true);
    }
    Ok(read)
}
