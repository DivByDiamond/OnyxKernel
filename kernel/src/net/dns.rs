use crate::net::poll;
use crate::net::udp;
use onyx_core::errno::{Errno, KResult};

const DNS_PORT: u16 = 53;

fn dns_encode_name(name: &[u8]) -> alloc::vec::Vec<u8> {
    let mut result = alloc::vec![];
    for part in name.split(|&b| b == b'.') {
        if !part.is_empty() {
            result.push(part.len() as u8);
            result.extend_from_slice(part);
        }
    }
    result.push(0);
    result
}

fn dns_skip_name(msg: &[u8], mut off: usize) -> Option<usize> {
    loop {
        if off >= msg.len() {
            return None;
        }
        let b = msg[off];
        if b == 0 {
            return Some(off + 1);
        }
        if b & 0xC0 == 0xC0 {
            return Some(off + 2);
        }
        off += 1 + b as usize;
    }
}

/// # Safety
///
/// Blocking resolver: binds a temp UDP socket and waits on poll(); must
/// run under the net single-poller contract and only where blocking is
/// acceptable (boot / kernel threads / a syscall handler with no locks
/// held), never in interrupt context. Uses the same SIE/wfi wait pattern
/// as `sys_nanosleep`, safe here too (even though this is several calls
/// deep under `sys_net_resolve`, not at the top of the syscall dispatch)
/// now that `srv::trap::handle` only reschedules off a trap that
/// interrupted user mode or an idle hart — see the guard comment there
/// (todo.md, wfi-deep-in-syscall page fault, 2026-09-03) for why a nested
/// kernel-mode wfi could not safely trigger a reschedule before that fix.
pub unsafe fn dns_resolve(hostname: &[u8], dns_server: [u8; 4]) -> KResult<[u8; 4]> {
    // SAFETY: only calls the udp_*/poll wrappers; reply buffer is a fixed 512-byte stack array, all reads bounds-checked.
    unsafe {
        let encoded = dns_encode_name(hostname);
        let qlen = encoded.len() + 4;
        let mut query = alloc::vec![0u8; 12 + qlen];
        // Random txid (hardware entropy): a predictable id (e.g. derived
        // from uptime) lets any local host forge replies. The reply-id match
        // below then discards everything we did not ask for.
        let id = crate::drivers::hwrand::next_u32() as u16;
        query[0..2].copy_from_slice(&id.to_be_bytes());
        query[2..4].copy_from_slice(&0x0100u16.to_be_bytes());
        query[4..6].copy_from_slice(&1u16.to_be_bytes());
        query[6..12].copy_from_slice(&[0; 6]);
        query[12..12 + encoded.len()].copy_from_slice(&encoded);
        let qoff = 12 + encoded.len();
        query[qoff..qoff + 2].copy_from_slice(&1u16.to_be_bytes());
        query[qoff + 2..qoff + 4].copy_from_slice(&1u16.to_be_bytes());
        // udp_bind_connect (not plain udp_bind + udp_sendto): the reply is
        // addressed back to the port the query was sent from, so sending
        // and receiving must go through the same bound socket.
        let sock = udp::udp_bind_connect(dns_server, DNS_PORT)?;
        udp::udp_send_bound(sock, &query)?;
        // Wall-clock deadline read straight off the `time` CSR (10 MHz on
        // QEMU virt, see CLINT_FREQ_QEMU): an iteration count is not a
        // reliable proxy here — on a fast host a busy-poll loop can burn
        // through tens of thousands of empty MMIO polls in well under a
        // millisecond, far faster than a real UDP round trip (ARP
        // resolution + the actual DNS query) ever completes.
        const TIME_HZ: u64 = crate::arch::regs::CLINT_FREQ_QEMU;
        let deadline = crate::arch::csr::read_time() + TIME_HZ * 5;
        while crate::arch::csr::read_time() < deadline {
            poll();
            // Re-arm this hart's timer and wait for the next interrupt
            // (matches sys_nanosleep's SIE/wfi pattern) instead of
            // busy-spinning poll() — see the safety doc above for why this
            // is safe from here now.
            #[cfg(not(test))]
            {
                crate::srv::timer::init_hart(crate::proc::process::hart_id());
                crate::arch::csr::set_sstatus(crate::arch::regs::SSTATUS_SIE);
                crate::arch::csr::wfi();
            }
            let mut buf = [0u8; 512];
            if let Ok(n) = udp::udp_recv(sock, &mut buf) {
                if n < 12 {
                    continue;
                }
                let rid = u16::from_be_bytes([buf[0], buf[1]]);
                if rid != id {
                    continue;
                }
                let flags = u16::from_be_bytes([buf[2], buf[3]]);
                if flags & 0x8000 == 0 {
                    continue;
                }
                if flags & 0x000F != 0 {
                    continue;
                }
                let qdcount = u16::from_be_bytes([buf[4], buf[5]]);
                let ancount = u16::from_be_bytes([buf[6], buf[7]]);
                if ancount == 0 {
                    continue;
                }
                let mut off = 12usize;
                for _ in 0..qdcount {
                    off = match dns_skip_name(&buf, off) {
                        Some(o) => o,
                        None => break,
                    };
                    off += 4;
                }
                for _ in 0..ancount {
                    off = match dns_skip_name(&buf, off) {
                        Some(o) => o,
                        None => break,
                    };
                    if off + 10 > n {
                        break;
                    }
                    let atype = u16::from_be_bytes([buf[off], buf[off + 1]]);
                    let aclass = u16::from_be_bytes([buf[off + 2], buf[off + 3]]);
                    let rdlength = u16::from_be_bytes([buf[off + 8], buf[off + 9]]) as usize;
                    off += 10;
                    if atype == 1 && aclass == 1 && rdlength == 4 && off + 4 <= n {
                        let ip = [buf[off], buf[off + 1], buf[off + 2], buf[off + 3]];
                        udp::udp_close(sock);
                        return Ok(ip);
                    }
                    off += rdlength;
                }
            }
        }
        udp::udp_close(sock);
        Err(Errno::Io)
    }
}
