//! Bounded passive TCP listeners; accepted connections use the ordinary socket API.
use super::{conn::{CONNS, TcpConn, BUF_SIZE, alloc_conn, alloc_local_port, send_tcp_seg}, state::{SYN,ACK}};
use crate::net::lock::{net_lock,net_unlock};
use onyx_core::errno::{Errno,KResult};
const LISTENERS_MAX: usize = 8;
const BACKLOG: usize = 4;
#[derive(Clone,Copy)]
struct Listener { owner_pid:u32, port:u16, queue:[usize;BACKLOG], first:usize, len:usize }
static mut LISTENERS: [Option<Listener>;LISTENERS_MAX] = [None;LISTENERS_MAX];

pub fn listen(port:u16) -> KResult<usize> {
    if port == 0 { return Err(Errno::Inval); }
    // SAFETY: all listener and socket operations are serialized by net_lock.
    unsafe {
        net_lock();
        let result = if LISTENERS.iter().flatten().any(|l|l.port==port) { Err(Errno::Busy) }
        else if let Some(index) = LISTENERS.iter().position(Option::is_none) {
            LISTENERS[index] = Some(Listener{owner_pid:0,port,queue:[0;BACKLOG],first:0,len:0}); Ok(index)
        } else { Err(Errno::Busy) };
        net_unlock(); result
    }
}

pub fn accept(index:usize) -> KResult<usize> {
    // SAFETY: bounds checked; all listener and socket operations under net_lock.
    unsafe {
        net_lock();
        let result = match LISTENERS.get_mut(index).and_then(Option::as_mut) {
            Some(listener) if listener.len>0 => {
                let cid=listener.queue[listener.first]; listener.first=(listener.first+1)%BACKLOG; listener.len-=1;
                if let Some(connection)=CONNS[cid].as_mut().filter(|c|c.listener==index && (c.state==2 || c.state==7)) { connection.listener=usize::MAX; Ok(cid) } else { Err(Errno::NoEnt) }
            },
            Some(_) => Err(Errno::NoEnt), None => Err(Errno::Inval),
        };
        net_unlock(); result
    }
}

pub fn close(index:usize) -> KResult<()> {
    // SAFETY: bounds checked; accepted sockets have detached listener ownership.
    unsafe {
        net_lock();
        let result=if index<LISTENERS_MAX && LISTENERS[index].is_some() {
            LISTENERS[index]=None;
            for slot in CONNS.iter_mut() { if slot.as_ref().is_some_and(|c|c.listener==index) { *slot=None; } }
            Ok(())
        } else { Err(Errno::Inval) };
        net_unlock();result
    }
}

/// # Safety
/// RX context under net_lock, after checksum and packet bounds validation.
pub(in crate::net) unsafe fn incoming_syn(port:u16, remote:[u8;4], remote_port:u16, seq:u32, window:u16) {
    // SAFETY: all indices checked or allocated from bounded tables.
    unsafe {
        let Some(parent)=LISTENERS.iter().position(|l|l.as_ref().is_some_and(|l|l.port==port)) else {return;};
        if CONNS.iter().flatten().filter(|c|c.listener==parent).count()>=BACKLOG {return;}
        let Some(cid)=alloc_conn() else {return;};
        let isn=u32::from(alloc_local_port()).wrapping_mul(65537).wrapping_add(seq);
        let mut connection=TcpConn {
            owner_pid:LISTENERS[parent].as_ref().map(|l|l.owner_pid).unwrap_or(0),
            iface:crate::drivers::virtio_net::active_device(), listener:parent,
            state:5,src_port:port,dst_ip:remote,dst_port:remote_port,
            snd_una:isn,snd_nxt:isn.wrapping_add(1),rcv_nxt:seq.wrapping_add(1),
            send_buf:[0;BUF_SIZE],send_len:0,recv_buf:[0;BUF_SIZE],recv_len:0,recv_head:0,
            peer_fin:false,tw_deadline_us:super::conn::now_us()+5_000_000,last_tx_us:super::conn::now_us(),retries:0,peer_window:window,
        };
        CONNS[cid]=Some(connection);
        connection.snd_nxt=isn;
        send_tcp_seg(&connection,SYN|ACK,&[]);
    }
}

/// # Safety
/// Caller holds net_lock; invoked once after passive handshake completion.
pub(in crate::net) unsafe fn established(cid:usize, parent:usize) {
    // SAFETY: parent and cid validated; listener may have closed while SYN was pending.
    unsafe {
        if let Some(listener)=LISTENERS.get_mut(parent).and_then(Option::as_mut)
            && listener.len<BACKLOG
        {
            listener.queue[(listener.first+listener.len)%BACKLOG]=cid;listener.len+=1;return;
        }
        CONNS[cid]=None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn listeners_reject_conflicting_ports_and_accept_is_nonblocking() {
        let _serial = crate::net::lock::test_serial();
        let index=listen(23456).expect("listen");
        assert_eq!(listen(23456),Err(Errno::Busy));
        assert_eq!(accept(index),Err(Errno::NoEnt));
        close(index).expect("close");
        assert_eq!(accept(index),Err(Errno::Inval));
    }
}

// Caller holds NET lock, serializing claims/checks with listener reuse.
pub(in crate::net) unsafe fn set_owner(index:usize,pid:u32) {
    unsafe { if let Some(listener)=LISTENERS.get_mut(index).and_then(Option::as_mut) {listener.owner_pid=pid;} }
}
pub(in crate::net) unsafe fn owner(index:usize)->Option<u32> {
    unsafe { LISTENERS.get(index).and_then(Option::as_ref).map(|l|l.owner_pid) }
}
