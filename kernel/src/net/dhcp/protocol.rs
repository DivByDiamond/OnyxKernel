use alloc::vec;

const DHCP_SERVER_PORT: u16 = 67;
pub(super) const DHCP_CLIENT_PORT: u16 = 68;

const DHCP_OP_BOOTREQUEST: u8 = 1;
const DHCP_OP_BOOTREPLY: u8 = 2;

const DHCP_MSG_DISCOVER: u8 = 1;
const DHCP_MSG_OFFER: u8 = 2;
const DHCP_MSG_REQUEST: u8 = 3;
const DHCP_MSG_ACK: u8 = 5;

const DHCP_OPT_PAD: u8 = 0;
const DHCP_OPT_SUBNET_MASK: u8 = 1;
const DHCP_OPT_ROUTER: u8 = 3;
const DHCP_OPT_DNS: u8 = 6;
const DHCP_OPT_REQUESTED_IP: u8 = 50;
const DHCP_OPT_MSG_TYPE: u8 = 53;
const DHCP_OPT_SERVER_ID: u8 = 54;
const DHCP_OPT_PARAM_LIST: u8 = 55;
const DHCP_OPT_END: u8 = 255;

const DHCP_HEADER_LEN: usize = 240;

fn make_dhcp_msg(
    msg_type: u8,
    xid: u32,
    mac: &[u8; 6],
    req_ip: Option<[u8; 4]>,
    server_id: Option<[u8; 4]>,
) -> alloc::vec::Vec<u8> {
    let mut pkt = alloc::vec![0u8; DHCP_HEADER_LEN + 32];
    pkt[0] = DHCP_OP_BOOTREQUEST;
    pkt[1] = 1;
    pkt[2] = 6;
    pkt[3] = 0;
    pkt[4..8].copy_from_slice(&xid.to_be_bytes());
    pkt[8..10].copy_from_slice(&[0, 0]);
    pkt[10..12].copy_from_slice(&0x8000u16.to_be_bytes());
    pkt[12..16].copy_from_slice(&[0; 4]);
    pkt[16..20].copy_from_slice(&[0; 4]);
    pkt[20..24].copy_from_slice(&[0; 4]);
    pkt[24..28].copy_from_slice(&[0; 4]);
    pkt[28..44].copy_from_slice(mac);
    pkt[44..236].fill(0);
    pkt[236..240].copy_from_slice(&[0x63, 0x82, 0x53, 0x63]);
    let mut off = 240;
    pkt[off] = DHCP_OPT_MSG_TYPE;
    pkt[off + 1] = 1;
    pkt[off + 2] = msg_type;
    off += 3;
    if let Some(ip) = req_ip {
        pkt[off] = DHCP_OPT_REQUESTED_IP;
        pkt[off + 1] = 4;
        pkt[off + 2..off + 6].copy_from_slice(&ip);
        off += 6;
    }
    if let Some(sid) = server_id {
        pkt[off] = DHCP_OPT_SERVER_ID;
        pkt[off + 1] = 4;
        pkt[off + 2..off + 6].copy_from_slice(&sid);
        off += 6;
    }
    if msg_type == DHCP_MSG_DISCOVER {
        pkt[off] = DHCP_OPT_PARAM_LIST;
        pkt[off + 1] = 3;
        pkt[off + 2] = DHCP_OPT_SUBNET_MASK;
        pkt[off + 3] = DHCP_OPT_ROUTER;
        pkt[off + 4] = DHCP_OPT_DNS;
        off += 5;
    }
    pkt[off] = DHCP_OPT_END;
    pkt.truncate(off + 1);
    pkt
}

fn parse_dhcp_reply(frame: &[u8]) -> Option<(u8, [u8; 4], [u8; 4], [u8; 4], [u8; 4])> {
    if frame.len() < DHCP_HEADER_LEN + 1 {
        return None;
    }
    let op = frame[0];
    if op != DHCP_OP_BOOTREPLY {
        return None;
    }
    let yiaddr: [u8; 4] = [frame[16], frame[17], frame[18], frame[19]];
    let siaddr: [u8; 4] = [frame[20], frame[21], frame[22], frame[23]];
    if frame[236..240] != [0x63, 0x82, 0x53, 0x63] {
        return None;
    }
    let mut msg_type = 0u8;
    let mut subnet_mask = [0u8; 4];
    let mut dns = [0u8; 4];
    let mut server_id = [0u8; 4];
    let mut got_server_id = false;
    let mut off = 240;
    loop {
        if off >= frame.len() {
            break;
        }
        let code = frame[off];
        if code == DHCP_OPT_END {
            break;
        }
        if code == DHCP_OPT_PAD {
            off += 1;
            continue;
        }
        if off + 1 >= frame.len() {
            break;
        }
        let len = frame[off + 1] as usize;
        if off + 2 + len > frame.len() {
            break;
        }
        match code {
            DHCP_OPT_MSG_TYPE if len >= 1 => msg_type = frame[off + 2],
            DHCP_OPT_SUBNET_MASK if len >= 4 => {
                subnet_mask.copy_from_slice(&frame[off + 2..off + 6]);
            }
            DHCP_OPT_DNS if len >= 4 => {
                dns.copy_from_slice(&frame[off + 2..off + 6]);
            }
            DHCP_OPT_SERVER_ID if len >= 4 => {
                server_id.copy_from_slice(&frame[off + 2..off + 6]);
                got_server_id = true;
            }
            _ => {}
        }
        off += 2 + len;
    }
    if msg_type == 0 {
        return None;
    }
    let sid = if got_server_id { server_id } else { siaddr };
    Some((msg_type, yiaddr, sid, subnet_mask, dns))
}
