//! TCP state-machine and socket-table tests.

use super::conn::{RECV_BUF_SIZE, SEND_BUF_SIZE, TcpConn};
use super::control::mss::TCP_MSS;

mod recv;
mod state;
mod table;

pub(super) fn test_conn() -> TcpConn {
    // State 1 = SYN_SENT, as left by tcp_connect().
    TcpConn {
        owner_pid: 0,
        iface: 0,
        listener: usize::MAX,
        last_tx_us: 0,
        retries: 0,
        peer_window: 2048,
        state: 1,
        src_port: 40000,
        dst_ip: [10, 0, 0, 1],
        dst_port: 80,
        snd_una: 1000,
        snd_nxt: 1001,
        rcv_nxt: 0,
        send_buf: [0; SEND_BUF_SIZE],
        send_len: 0,
        recv_buf: [0; RECV_BUF_SIZE],
        recv_len: 0,
        recv_head: 0,
        peer_fin: false,
        tw_deadline_us: 0,
        peer_mss: TCP_MSS as u16,
    }
}
