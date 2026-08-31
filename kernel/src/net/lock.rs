//! Recursive NET lock — the single serialization point of the network stack.
//!
//! Net sync fix (todo P1 #4 network-without-synchronization + P1 #6 reentrancy).
//!
//! ## What it protects
//!
//! Every cross-hart shared static of the net stack, previously mutated with
//! no lock at all:
//!
//! | Guarded state               | File            |
//! |-----------------------------|-----------------|
//! | `UDP_SOCKS`, `NEXT_UDP_PORT`| net/udp.rs      |
//! | `CONNS`, `NEXT_PORT`        | net/tcp/conn.rs |
//! | `ARP_CACHE_*`               | net/eth.rs      |
//! | `IP_ID`                     | net/ip.rs       |
//! | virtio-net TX/RX ring access (G_NET) | drivers/virtio_net |
//!
//! The G_IP/G_GW/G_MASK configuration statics stay lock-free: they are
//! written once at boot before secondary harts are released and only read
//! afterwards.
//!
//! ## Why RECURSIVE
//!
//! The call graph nests lock acquisitions on the same hart:
//!
//! ```text
//! udp_send / tcp_send / handle_icmp          (acquire 1)
//!   └─ ip::send_packet                       (acquire 2)
//!        └─ ARP miss: arp_request + poll     (acquire 3)
//!             └─ eth::dispatch               (acquire 4)
//!                  ├─ handle_arp → arp_insert            (acquire 5)
//!                  ├─ handle_udp → UDP_SOCKS ring write  (acquire 5)
//!                  └─ handle_tcp → CONNS mutation        (acquire 5)
//! ```
//!
//! A plain SpinLock would self-deadlock at the second acquisition. The
//! classic alternative — dropping the lock around `poll()` — would instead
//! reintroduce exactly the race this lock exists to close. A per-hart
//! recursive lock keeps exclusivity (only the owner hart may nest) while
//! making reentrancy a no-op.
//!
//! ## Interrupt invariant
//!
//! Same discipline as `crate::sync::SpinLock`: acquire only in kernel
//! context with SIE clear (guaranteed by trap_return for every kernel
//! path). No idle-loop code takes this lock, so there is no SIE-set context
//! that could observe it held. Blocking inside the lock (polling waits in
//! `tcp_connect`/`send_packet`/`virtio_net::send`) serializes other harts'
//! net operations for the duration — accepted cost, net ops are short and
//! the previous implicit contract already required single-poller/single-
//! sender execution.
//!
//! ## Lock ordering
//!
//! NET_LOCK is a leaf lock: it never nests inside itself beyond recursion,
//! and no code takes PROC_LIST_LOCK / rq_locks / G_QLOCK / heap or pmm
//! locks while holding it *and* vice versa (heap/pmm allocations inside
//! `virtio_net::send` acquire their own leaf locks while NET_LOCK is held —
//! nothing acquires NET_LOCK while holding those).

use core::sync::atomic::{AtomicUsize, Ordering};

use crate::proc::hart_id;
use crate::sync::SpinLock;

const NO_OWNER: usize = usize::MAX;

struct NetLock {
    /// Underlying exclusive spinlock (held by the single owner hart).
    lock: SpinLock,
    /// Hart id of the current owner (NO_OWNER when free).
    owner: AtomicUsize,
    /// Nesting depth of the owner (>= 1 while held).
    depth: AtomicUsize,
}

// SAFETY: the lock state is atomic; guarded data lives outside the lock in
// `static mut` state accessed only while holding it, as with crate::sync::SpinLock.
unsafe impl Sync for NetLock {}

static NET_LOCK: NetLock = NetLock {
    lock: SpinLock::new(),
    owner: AtomicUsize::new(NO_OWNER),
    depth: AtomicUsize::new(0),
};

/// Acquire the NET lock, recursively (no-op acquire when the calling hart
/// already owns it). See the module docs for the interrupt invariant.
pub fn net_lock() {
    let id = hart_id();
    // Fast path: this hart already owns the lock — bump the depth. Only the
    // owner hart ever writes its own id, and a hart observes its own writes
    // in program order, so this read cannot produce a false positive.
    if NET_LOCK.owner.load(Ordering::Relaxed) == id {
        NET_LOCK.depth.fetch_add(1, Ordering::Relaxed);
        return;
    }
    NET_LOCK.lock.lock();
    NET_LOCK.owner.store(id, Ordering::Release);
    NET_LOCK.depth.store(1, Ordering::Release);
}

/// Release one level of NET lock nesting; the underlying spinlock is only
/// dropped when the depth reaches zero. Must pair every `net_lock()` 1:1.
pub fn net_unlock() {
    let d = NET_LOCK.depth.fetch_sub(1, Ordering::AcqRel);
    if d == 1 {
        NET_LOCK.owner.store(NO_OWNER, Ordering::Release);
        NET_LOCK.lock.unlock();
    }
}

/// Test-only introspection: whether the calling (host) hart currently owns
/// the lock and at which depth.
#[cfg(test)]
pub fn owned_depth() -> usize {
    if NET_LOCK.owner.load(Ordering::Relaxed) == hart_id() {
        NET_LOCK.depth.load(Ordering::Relaxed)
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Combined test: NET_LOCK is a process-global static and the host test
    /// harness runs #[test] fns in parallel, so both nesting and
    /// release/reacquire assertions live in one function to stay
    /// deterministic (same pattern as runqueue.rs).
    #[test]
    fn test_recursive_lock_nesting_and_reacquire() {
        // Host test: hart_id() == 0, single thread — exercise the recursion.
        net_lock();
        assert_eq!(owned_depth(), 1);
        net_lock();
        net_lock();
        assert_eq!(owned_depth(), 3);
        // Depth must unwind exactly; the underlying lock is only free at 0.
        net_unlock();
        net_unlock();
        assert_eq!(owned_depth(), 1);
        net_unlock();
        assert_eq!(owned_depth(), 0);
        // Fully released — acquire again cleanly.
        net_lock();
        assert_eq!(owned_depth(), 1);
        net_unlock();
        assert_eq!(owned_depth(), 0);
    }
}
