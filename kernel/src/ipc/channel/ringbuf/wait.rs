//! Sleep/wake list management for channel senders and receivers.

use crate::ipc::channel::types::G_CHANNELS;

/// # Safety: caller must hold the channel spinlock owning `*wait_head`; `current()` must be the live Proc of the calling context (its state/wait_next are mutated under that lock).
pub(super) unsafe fn wait_enqueue(wait_head: &mut *mut crate::proc::Proc) {
    unsafe {
        let p = crate::proc::current() as *mut crate::proc::Proc;
        (*p).state = crate::proc::ProcState::Waiting;
        (*p).wait_next = *wait_head;
        *wait_head = p;
    }
}

/// # Safety: caller must hold the channel spinlock owning `*wait_head`; every pointer was enqueued by `wait_enqueue` and is still a valid Proc (waiters are only cleared here).
pub(super) unsafe fn wait_wake_all(wait_head: &mut *mut crate::proc::Proc) {
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

/// Remove a dying process from a channel's sleep-wake list.
///
/// SMP respawn-crash fix v4 (2026-09-08): `wait_enqueue` stores the caller's
/// raw `*mut Proc` in `send_wait`/`recv_wait`, but NOTHING unlinked the
/// entry when the waiter died — `exit()` never walked these lists, so a
/// channel kept a dangling `Proc*` across reaps. The reaped node's memory is
/// recycled by the next `alloc_proc`, and the NEXT wake on that channel
/// (`wait_wake_all`) then wrote `state = Ready` / `wait_next = null` through
/// the freed pointer — corrupting whatever live process now occupies that
/// slot (its `state` field sits at offset 5, next at ~offset 65560). With
/// the respawn storm recycling ~200 Procs/s through init's request/response
/// channels, a corrupted `tf`/`state` on the recycled node resurfaced as the
/// "kernel-mode jump into the dead child's text" crash family. Called from
/// `proc::exit` for every channel slot while holding that channel's lock;
/// the dying process cannot be running or waiting elsewhere by then.
///
/// # Safety
///
/// `p` must be the exiting Proc pointer (never dereferenced here beyond
/// pointer comparison — no field access, so it stays safe even if the node
/// were somehow already recycled); each channel's lock is taken inside.
pub unsafe fn disconnect_waiter(p: *mut crate::proc::Proc) {
    unsafe {
        for ch in G_CHANNELS.iter_mut() {
            ch.lock.lock();
            // Unlink from recv_wait.
            let mut cur = ch.recv_wait;
            let mut prev: *mut crate::proc::Proc = core::ptr::null_mut();
            while !cur.is_null() {
                let next = (*cur).wait_next;
                if cur == p {
                    if prev.is_null() {
                        ch.recv_wait = next;
                    } else {
                        (*prev).wait_next = next;
                    }
                } else {
                    prev = cur;
                }
                cur = next;
            }
            // Unlink from send_wait.
            cur = ch.send_wait;
            prev = core::ptr::null_mut();
            while !cur.is_null() {
                let next = (*cur).wait_next;
                if cur == p {
                    if prev.is_null() {
                        ch.send_wait = next;
                    } else {
                        (*prev).wait_next = next;
                    }
                } else {
                    prev = cur;
                }
                cur = next;
            }
            ch.lock.unlock();
        }
    }
}
