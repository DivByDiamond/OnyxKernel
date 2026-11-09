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
mod tests;
