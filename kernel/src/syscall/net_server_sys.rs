//! Privileged server socket operations for the bounded guest TCP/UDP stack.
use super::handler::user_ptr_ok;
use crate::{mm::vmm,net,proc};
use onyx_core::errno::Errno;

/// # Safety
/// Syscall context. No user pointers; port is checked before conversion.
pub(super) unsafe fn sys_net_listen(port:u64) -> i64 {
    if port==0 || port>65535 {return Errno::Inval.as_i64();}
    net::ownership::listen(port as u16,proc::current_pid()).map_or_else(Errno::as_i64,|id|id as i64)
}
/// # Safety
/// Syscall context; accept is nonblocking after a bounded poll pass.
pub(super) unsafe fn sys_net_accept(id:u64) -> i64 {
    // SAFETY: net::poll serializes RX and accept validates the listener ID.
    unsafe { net::poll(); }
    net::ownership::listener(id as usize,proc::current_pid(),||net::tcp::listen::accept(id as usize)).map_or_else(Errno::as_i64,|cid|cid as i64)
}
/// # Safety
/// Syscall context; the listener API validates the ID.
pub(super) unsafe fn sys_net_unlisten(id:u64) -> i64 {
    net::ownership::listener(id as usize,proc::current_pid(),||net::tcp::listen::close(id as usize)).map_or_else(Errno::as_i64,|()|0)
}
/// # Safety
/// Syscall context; all user ranges are validated before dequeueing a datagram.
pub(super) unsafe fn sys_udp_recvfrom(id:u64,ptr:u64,len:u64,peer:u64) -> i64 {
    if id>=8 || len>1472 || !user_ptr_ok(ptr,len) || !user_ptr_ok(peer,8) {return Errno::Inval.as_i64();}
    // SAFETY: writable pages checked first, copy helpers revalidate each page.
    unsafe {
        let root=proc::current().root_pa;
        if vmm::check_user_range(root,ptr,len,true).is_err() || vmm::check_user_range(root,peer,8,true).is_err() {return Errno::Fault.as_i64();}
        let mut buffer=[0;1472];net::poll();
        match net::ownership::udp(id as usize,proc::current_pid(),||net::udp::udp_recv_from(id as usize,&mut buffer[..len as usize])) {
            Ok((n,remote))=>{
                let mut metadata=[0;8];metadata[..4].copy_from_slice(&remote.ip);
                metadata[4..6].copy_from_slice(&remote.port.to_be_bytes());metadata[6]=remote.iface;
                if vmm::copy_to_user(root,ptr,buffer.as_ptr(),n).is_err() || vmm::copy_to_user(root,peer,metadata.as_ptr(),8).is_err() {return Errno::Fault.as_i64();}
                n as i64
            },Err(error)=>error.as_i64(),
        }
    }
}
/// # Safety
/// Syscall context; endpoint and bounded data copied from checked user pages.
pub(super) unsafe fn sys_udp_sendfrom(id:u64,peer:u64,ptr:u64,len:u64) -> i64 {
    if id>=8 || len>1472 || !user_ptr_ok(ptr,len) || !user_ptr_ok(peer,8) {return Errno::Inval.as_i64();}
    // SAFETY: copy helpers validate each user page before reading.
    unsafe {
        let root=proc::current().root_pa;let mut metadata=[0;8];let mut data=[0;1472];
        if vmm::copy_from_user(root,metadata.as_mut_ptr(),peer,8).is_err() || vmm::copy_from_user(root,data.as_mut_ptr(),ptr,len as usize).is_err() {return Errno::Fault.as_i64();}
        let remote=net::udp::datagram::Peer {ip:metadata[..4].try_into().unwrap_or([0;4]),port:u16::from_be_bytes([metadata[4],metadata[5]]),iface:metadata[6]};
        if remote.port==0 {return Errno::Inval.as_i64();}
        net::ownership::udp(id as usize,proc::current_pid(),||net::udp::udp_send_from(id as usize,remote,&data[..len as usize])).map_or_else(Errno::as_i64,|()|len as i64)
    }
}
