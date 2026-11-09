use super::*;
#[test]
fn listeners_reject_conflicting_ports_and_accept_is_nonblocking() {
    let _serial = crate::net::lock::test_serial();
    let index = listen(23456).expect("listen");
    assert_eq!(listen(23456), Err(Errno::Busy));
    assert_eq!(accept(index), Err(Errno::NoEnt));
    close(index).expect("close");
    assert_eq!(accept(index), Err(Errno::Inval));
}
