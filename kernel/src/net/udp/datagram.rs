//! Bounded UDP datagram queue: packet boundaries and sender metadata survive RX.
pub const QUEUE_BYTES: usize = 2048;
const QUEUE_MESSAGES: usize = 8;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Peer { pub ip: [u8;4], pub port: u16, pub iface: u8 }
const EMPTY_PEER: Peer = Peer { ip: [0;4], port: 0, iface: 0 };
#[derive(Clone, Copy)]
pub struct DatagramQueue {
    bytes: [u8; QUEUE_BYTES], head: usize, used: usize,
    lengths: [usize; QUEUE_MESSAGES], peers: [Peer; QUEUE_MESSAGES], first: usize, messages: usize,
}
impl DatagramQueue {
    pub const EMPTY: Self = Self { bytes: [0;QUEUE_BYTES], head:0, used:0, lengths:[0;QUEUE_MESSAGES], peers:[EMPTY_PEER;QUEUE_MESSAGES], first:0, messages:0 };
    pub fn push(&mut self, payload: &[u8], peer: Peer) -> bool {
        if payload.len() > QUEUE_BYTES - self.used || self.messages == QUEUE_MESSAGES { return false; }
        let tail = (self.head + self.used) % QUEUE_BYTES;
        for (index, byte) in payload.iter().enumerate() { self.bytes[(tail+index)%QUEUE_BYTES] = *byte; }
        let slot = (self.first+self.messages)%QUEUE_MESSAGES;
        self.lengths[slot] = payload.len(); self.peers[slot] = peer;
        self.used += payload.len(); self.messages += 1; true
    }
    pub fn pop(&mut self, target: &mut [u8]) -> Option<(usize, Peer)> {
        if self.messages == 0 { return None; }
        let length = self.lengths[self.first]; let n = length.min(target.len());
        for (index, byte) in target[..n].iter_mut().enumerate() { *byte = self.bytes[(self.head+index)%QUEUE_BYTES]; }
        let peer = self.peers[self.first];
        self.head = (self.head+length)%QUEUE_BYTES; self.used -= length;
        self.first = (self.first+1)%QUEUE_MESSAGES; self.messages -= 1;
        Some((n,peer))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn messages_do_not_merge_and_truncation_discards_only_one_datagram() {
        let _serial = crate::net::lock::test_serial();
        let mut q = DatagramQueue::EMPTY; let p = Peer { ip:[192,168,50,2],port:123,iface:1 };
        assert!(q.push(b"hello",p)); assert!(q.push(b"world",p));
        let mut small = [0;2]; assert_eq!(q.pop(&mut small),Some((2,p))); assert_eq!(&small,b"he");
        let mut out = [0;8]; assert_eq!(q.pop(&mut out),Some((5,p))); assert_eq!(&out[..5],b"world");
        assert_eq!(q.pop(&mut out),None);
    }
    #[test]
    fn empty_datagrams_and_full_queue_are_bounded() {
        let _serial = crate::net::lock::test_serial();
        let mut q = DatagramQueue::EMPTY;
        for _ in 0..8 { assert!(q.push(&[],EMPTY_PEER)); }
        assert!(!q.push(b"overflow",EMPTY_PEER));
        assert_eq!(q.pop(&mut []),Some((0,EMPTY_PEER)));
        assert!(q.push(b"ok",EMPTY_PEER));
    }
}
