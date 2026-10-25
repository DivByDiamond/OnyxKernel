//! Bounded retransmission plans; no unbounded retry loops or retained socket borrows.
use super::{conn::{TcpConn,CONNS,send_tcp_seg_tx},state::{SYN,ACK,FIN}};
const RETRY_US:u64=1_000_000;
const RETRIES:u8=5;
#[derive(Debug,Clone,Copy,PartialEq,Eq)]
pub(super) struct Plan {pub flags:u8,pub sequence:u32,pub length:usize}
pub(super) fn plan(connection:&mut TcpConn,now:u64)->Option<Plan>{
    let action=match connection.state{
        1=>Plan{flags:SYN,sequence:connection.snd_una,length:0},
        5=>Plan{flags:SYN|ACK,sequence:connection.snd_una,length:0},
        2 | 7 if connection.send_len>0=>Plan{flags:0x18,sequence:connection.snd_una,length:connection.send_len.min(536)},
        3 if connection.send_len>0=>Plan{flags:0x18,sequence:connection.snd_una,length:connection.send_len.min(536)},
        3=>Plan{flags:FIN|ACK,sequence:connection.snd_nxt.wrapping_sub(1),length:0},
        _=>return None,
    };
    if now.wrapping_sub(connection.last_tx_us)<RETRY_US{return None;}
    if connection.retries>=RETRIES{connection.state=6;connection.tw_deadline_us=now+5_000_000;return None;}
    connection.retries+=1;connection.last_tx_us=now;Some(action)
}
static mut RUNNING:bool=false;
/// # Safety
/// Caller holds net_lock; avoids recursively retrying while TX resolves ARP.
pub(super) unsafe fn tick(now:u64){
    // SAFETY: network lock serializes static/table access. Plans and segment
    // copies are collected first: send_tcp_seg re-enters TX (ARP/poll) and can
    // mutate CONNS, so no slot borrow may stay live across it.
    unsafe{
        if RUNNING{return;}RUNNING=true;
        // Collect (slot, plan) pairs only: a `TcpConn` value is ~10 KB and
        // copying it into the pending vector overflows the 64 KiB kstack
        // (see `TcpTx`). The payload is re-read per entry below.
        let mut pending=alloc::vec::Vec::new();
        for (cid,connection) in CONNS.iter_mut().enumerate().filter_map(|(i,c)|c.as_mut().map(|c|(i,c))){
            if let Some(action)=plan(connection,now){
                connection.snd_nxt=action.sequence;
                pending.push((cid,action));
            }
        }
        for (cid,action) in pending{
            let Some(connection)=CONNS[cid].as_ref() else {continue;};
            let tx=connection.tx();
            let length=action.length.min(536);
            let mut payload=[0u8;536];
            payload[..length].copy_from_slice(&connection.send_buf[..length]);
            send_tcp_seg_tx(tx,action.flags,&payload[..length]);
        }
        RUNNING=false;
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn retransmits_only_unacknowledged_prefix_and_eventually_reports_failure(){
        let _serial = crate::net::lock::test_serial();
        let mut connection=super::super::tests::test_conn();connection.state=2;
        connection.snd_una=100;connection.snd_nxt=103;connection.send_len=3;
        assert_eq!(plan(&mut connection,999_999),None);
        for retry in 1..=5{
            assert_eq!(plan(&mut connection,retry*RETRY_US),Some(Plan{flags:0x18,sequence:100,length:3}));
        }
        assert_eq!(plan(&mut connection,6*RETRY_US),None);assert_eq!(connection.state,6);
    }
}
