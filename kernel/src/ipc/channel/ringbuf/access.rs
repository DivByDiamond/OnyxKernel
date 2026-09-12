//! Channel access control: who is allowed to send/recv on a given channel.

use crate::ipc::channel::types::Channel;

pub(super) fn pid_allowed(ch: &Channel, pid: u32) -> bool {
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

#[cfg(test)]
mod tests {
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
}
