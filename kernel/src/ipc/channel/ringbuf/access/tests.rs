use super::*;

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
