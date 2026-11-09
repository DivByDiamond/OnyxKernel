use super::*;
#[test]
fn process_exit_releases_owned_ports_and_preserves_other_processes() {
    let _serial = crate::net::lock::test_serial();
    unsafe {
        net_lock();
        let saved = tcp::conn::CONNS;
        let listener_id = listen(34570, 41).unwrap();
        let udp_id = bind_udp(34571, 41).unwrap();
        let mut own: tcp::conn::TcpConn = core::mem::zeroed();
        own.owner_pid = 41;
        own.listener = usize::MAX;
        let mut foreign = own;
        foreign.owner_pid = 42;
        tcp::conn::CONNS[0] = Some(own);
        tcp::conn::CONNS[1] = Some(foreign);
        close_process(41);
        assert!(tcp::conn::CONNS[0].is_none());
        assert_eq!(tcp::conn::CONNS[1].unwrap().owner_pid, 42);
        assert_eq!(listener(listener_id, 41, || Ok(1)), Err(Errno::Inval));
        assert_eq!(udp(udp_id, 41, || Ok(1)), Err(Errno::Inval));
        let new_listener = listen(34570, 42).unwrap();
        let new_udp = bind_udp(34571, 42).unwrap();
        tcp::listen::close(new_listener).unwrap();
        crate::net::udp::udp_close(new_udp);
        tcp::conn::CONNS = saved;
        net_unlock();
    }
}
#[test]
fn accepted_connections_inherit_listener_process_owner() {
    let _serial = crate::net::lock::test_serial();
    unsafe {
        net_lock();
        let saved = tcp::conn::CONNS;
        let id = listen(34569, 41).unwrap();
        tcp::listen::incoming_syn(34569, [192, 168, 50, 2], 40001, 100, 2048, 1460);
        let cid = tcp::conn::CONNS
            .iter()
            .position(|slot| slot.as_ref().is_some_and(|c| c.listener == id))
            .unwrap();
        assert_eq!(tcp::conn::CONNS[cid].unwrap().owner_pid, 41);
        tcp::conn::CONNS[cid].as_mut().unwrap().state = 2;
        tcp::listen::established(cid, id);
        assert_eq!(
            listener(id, 42, || tcp::listen::accept(id)),
            Err(Errno::Perm)
        );
        assert_eq!(listener(id, 41, || tcp::listen::accept(id)), Ok(cid));
        assert_eq!(tcp(cid, 42, || Ok(7)), Err(Errno::Perm));
        assert_eq!(tcp(cid, 41, || Ok(7)), Ok(7));
        tcp::listen::close(id).unwrap();
        tcp::conn::CONNS = saved;
        net_unlock();
    }
}
#[test]
fn sockets_and_listeners_reject_other_processes() {
    let _serial = crate::net::lock::test_serial();
    let listener_id = listen(34567, 41).unwrap();
    assert_eq!(listener(listener_id, 42, || Ok(7)), Err(Errno::Perm));
    assert_eq!(listener(listener_id, 41, || Ok(7)), Ok(7));
    listener(listener_id, 41, || tcp::listen::close(listener_id)).unwrap();
    let udp_id = bind_udp(34568, 41).unwrap();
    assert_eq!(udp(udp_id, 42, || Ok(7)), Err(Errno::Perm));
    assert_eq!(udp(udp_id, 41, || Ok(7)), Ok(7));
    udp(udp_id, 41, || {
        unsafe { crate::net::udp::udp_close(udp_id) };
        Ok(())
    })
    .unwrap();
    unsafe {
        net_lock();
        let saved = tcp::conn::CONNS;
        let mut connection: tcp::conn::TcpConn = core::mem::zeroed();
        connection.owner_pid = 41;
        tcp::conn::CONNS[0] = Some(connection);
        assert_eq!(tcp(0, 42, || Ok(7)), Err(Errno::Perm));
        assert_eq!(tcp(0, 41, || Ok(7)), Ok(7));
        tcp::conn::CONNS = saved;
        net_unlock();
    }
}
