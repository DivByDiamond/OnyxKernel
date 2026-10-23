//! User-visible numeric socket IDs are process-owned capabilities. Check and
//! operate under one NET lock, excluding close/reallocation between both steps.
use onyx_core::errno::{Errno,KResult};
use super::{lock::{net_lock,net_unlock},tcp,udp};
fn serialized<T>(operation:impl FnOnce()->KResult<T>)->KResult<T> {
    net_lock();let result=operation();net_unlock();result
}
pub fn tcp_connect(ip:[u8;4],port:u16,pid:u32)->KResult<usize> {
    serialized(||unsafe {let id=tcp::tcp_connect(ip,port)?;tcp::conn::CONNS[id].as_mut().ok_or(Errno::Inval)?.owner_pid=pid;Ok(id)})
}
pub fn tcp<T>(id:usize,pid:u32,operation:impl FnOnce()->KResult<T>)->KResult<T> {
    serialized(||unsafe {
        match tcp::conn::CONNS.get(id).and_then(Option::as_ref) {
            Some(socket) if socket.owner_pid==pid=>operation(),
            Some(_)=>Err(Errno::Perm),None=>Err(Errno::Inval),
        }
    })
}
pub fn listen(port:u16,pid:u32)->KResult<usize> {
    serialized(||unsafe {let id=tcp::listen::listen(port)?;tcp::listen::set_owner(id,pid);Ok(id)})
}
pub fn listener<T>(id:usize,pid:u32,operation:impl FnOnce()->KResult<T>)->KResult<T> {
    serialized(||unsafe {match tcp::listen::owner(id) {
        Some(owner) if owner==pid=>operation(),Some(_)=>Err(Errno::Perm),None=>Err(Errno::Inval),
    }})
}
pub fn bind_udp(port:u16,pid:u32)->KResult<usize> {
    serialized(||unsafe {let id=udp::udp_bind_any(port)?;udp::UDP_SOCKS[id].as_mut().ok_or(Errno::Inval)?.owner_pid=pid;Ok(id)})
}
pub fn udp<T>(id:usize,pid:u32,operation:impl FnOnce()->KResult<T>)->KResult<T> {
    serialized(||unsafe {match udp::UDP_SOCKS.get(id).and_then(Option::as_ref) {
        Some(socket) if socket.owner_pid==pid=>operation(),Some(_)=>Err(Errno::Perm),None=>Err(Errno::Inval),
    }})
}
/// Called before process teardown takes process-list or runqueue locks.
/// Bounded table cleanup does no packet IO, allocation or scheduler work.
pub fn close_process(pid:u32) {
    net_lock();
    unsafe {
        for id in 0..8 { if tcp::listen::owner(id)==Some(pid) {let _=tcp::listen::close(id);} }
        for slot in tcp::conn::CONNS.iter_mut() {
            if slot.as_ref().is_some_and(|s|s.owner_pid==pid) {
                if let Some(socket)=slot.as_mut().filter(|s|s.state==3 || s.state==4) {
                    socket.owner_pid=0; // Kernel retains bounded FIN/data retries after close.
                } else {*slot=None;}
            }
        }
        for slot in udp::UDP_SOCKS.iter_mut() {if slot.as_ref().is_some_and(|s|s.owner_pid==pid) {*slot=None;} }
    }
    net_unlock();
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn process_exit_releases_owned_ports_and_preserves_other_processes() {
        let _serial = crate::net::lock::test_serial();
        unsafe {
            net_lock();let saved=tcp::conn::CONNS;
            let listener_id=listen(34570,41).unwrap();
            let udp_id=bind_udp(34571,41).unwrap();
            let mut own:tcp::conn::TcpConn=core::mem::zeroed();own.owner_pid=41;own.listener=usize::MAX;
            let mut foreign=own;foreign.owner_pid=42;
            tcp::conn::CONNS[0]=Some(own);tcp::conn::CONNS[1]=Some(foreign);
            close_process(41);
            assert!(tcp::conn::CONNS[0].is_none());
            assert_eq!(tcp::conn::CONNS[1].unwrap().owner_pid,42);
            assert_eq!(listener(listener_id,41,||Ok(1)),Err(Errno::Inval));
            assert_eq!(udp(udp_id,41,||Ok(1)),Err(Errno::Inval));
            let new_listener=listen(34570,42).unwrap();let new_udp=bind_udp(34571,42).unwrap();
            tcp::listen::close(new_listener).unwrap();super::super::udp::udp_close(new_udp);
            tcp::conn::CONNS=saved;net_unlock();
        }
    }
    #[test]
    fn accepted_connections_inherit_listener_process_owner() {
        let _serial = crate::net::lock::test_serial();
        unsafe {
            net_lock();let saved=tcp::conn::CONNS;
            let id=listen(34569,41).unwrap();
            tcp::listen::incoming_syn(34569,[192,168,50,2],40001,100,2048);
            let cid=tcp::conn::CONNS.iter().position(|slot|slot.as_ref().is_some_and(|c|c.listener==id)).unwrap();
            assert_eq!(tcp::conn::CONNS[cid].unwrap().owner_pid,41);
            tcp::conn::CONNS[cid].as_mut().unwrap().state=2;
            tcp::listen::established(cid,id);
            assert_eq!(listener(id,42,||tcp::listen::accept(id)),Err(Errno::Perm));
            assert_eq!(listener(id,41,||tcp::listen::accept(id)),Ok(cid));
            assert_eq!(tcp(cid,42,||Ok(7)),Err(Errno::Perm));
            assert_eq!(tcp(cid,41,||Ok(7)),Ok(7));
            tcp::listen::close(id).unwrap();tcp::conn::CONNS=saved;net_unlock();
        }
    }
    #[test]
    fn sockets_and_listeners_reject_other_processes() {
        let _serial = crate::net::lock::test_serial();
        let listener_id=listen(34567,41).unwrap();
        assert_eq!(listener(listener_id,42,||Ok(7)),Err(Errno::Perm));
        assert_eq!(listener(listener_id,41,||Ok(7)),Ok(7));
        listener(listener_id,41,||tcp::listen::close(listener_id)).unwrap();
        let udp_id=bind_udp(34568,41).unwrap();
        assert_eq!(udp(udp_id,42,||Ok(7)),Err(Errno::Perm));
        assert_eq!(udp(udp_id,41,||Ok(7)),Ok(7));
        udp(udp_id,41,||{unsafe {super::super::udp::udp_close(udp_id)};Ok(())}).unwrap();
        unsafe {
            net_lock();let saved=tcp::conn::CONNS;
            let mut connection:tcp::conn::TcpConn=core::mem::zeroed();connection.owner_pid=41;
            tcp::conn::CONNS[0]=Some(connection);
            assert_eq!(tcp(0,42,||Ok(7)),Err(Errno::Perm));
            assert_eq!(tcp(0,41,||Ok(7)),Ok(7));
            tcp::conn::CONNS=saved;net_unlock();
        }
    }
}
