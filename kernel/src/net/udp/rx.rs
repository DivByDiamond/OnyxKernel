use super::{UDP_HLEN, UDP_SOCKS, datagram::Peer};
use crate::net::{checksum::pseudo_checksum, lock::{net_lock,net_unlock}};

/// # Safety
/// RX context selected under net_lock; this function revalidates all lengths.
pub unsafe fn handle_udp(frame: &[u8], ip_start: usize) {
    if frame.len() < ip_start + 20 { return; }
    let header_len = usize::from(frame[ip_start] & 15) * 4;
    let offset = ip_start + header_len;
    if header_len < 20 || frame.len() < offset + UDP_HLEN { return; }
    let length = usize::from(u16::from_be_bytes([frame[offset+4],frame[offset+5]]));
    if length < UDP_HLEN || offset + length > frame.len() { return; }
    let source: [u8;4] = frame[ip_start+12..ip_start+16].try_into().unwrap_or([0;4]);
    let destination: [u8;4] = frame[ip_start+16..ip_start+20].try_into().unwrap_or([0;4]);
    if frame[offset+6..offset+8] != [0,0] && pseudo_checksum(&source,&destination,17,&frame[offset..offset+length]) != 0 { return; }
    let source_port = u16::from_be_bytes([frame[offset],frame[offset+1]]);
    let destination_port = u16::from_be_bytes([frame[offset+2],frame[offset+3]]);
    // SAFETY: all packet slices validated above; socket updates under net_lock.
    unsafe {
        net_lock();
        let iface = crate::drivers::virtio_net::active_device();
        for socket in UDP_SOCKS.iter_mut().flatten() {
            if socket.bound && socket.local_port == destination_port
                && (socket.iface == iface || socket.iface == usize::MAX)
                && (!socket.connected || (socket.remote_ip == source && socket.remote_port == source_port)) {
                socket.datagrams.push(&frame[offset+UDP_HLEN..offset+length],Peer{ip:source,port:source_port,iface:iface as u8});
                break;
            }
        }
        net_unlock();
    }
}
