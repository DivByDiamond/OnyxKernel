use super::super::{MAX_MSG_LEN, REQ_CHANNEL, RESP_CHANNEL};
use crate::syscalls;

pub(crate) unsafe fn control_main(argc: usize, argv: *const u64) -> ! {
    if argc < 2 {
        let m = b"Usage: init <status|start|stop|enable|disable|list> [service]\n";
        syscalls::write(1, m.as_ptr(), m.len());
        syscalls::exit(1);
    }

    let cmd = read_argv(argv, 1);
    let arg = if argc >= 3 { read_argv(argv, 2) } else { &[] };

    let valid = cmd == b"status"
        || cmd == b"start"
        || cmd == b"stop"
        || cmd == b"enable"
        || cmd == b"disable"
        || cmd == b"list";
    if !valid {
        let m = b"init: unknown command '";
        syscalls::write(1, m.as_ptr(), m.len());
        syscalls::write(1, cmd.as_ptr(), cmd.len());
        let m = b"'\nUsage: init <status|start|stop|enable|disable|list> [service]\n";
        syscalls::write(1, m.as_ptr(), m.len());
        syscalls::exit(1);
    }

    if cmd != b"list" && arg.is_empty() {
        let m = b"init: missing service name\n";
        syscalls::write(1, m.as_ptr(), m.len());
        syscalls::exit(1);
    }

    let mut req = [0u8; MAX_MSG_LEN];
    let mut pos = 0;
    let cl = cmd.len().min(req.len() - 2);
    req[pos..pos + cl].copy_from_slice(&cmd[..cl]);
    pos += cl;
    if !arg.is_empty() {
        req[pos] = b' ';
        pos += 1;
        let al = arg.len().min(req.len() - pos - 1);
        req[pos..pos + al].copy_from_slice(&arg[..al]);
        pos += al;
    }
    req[pos] = b'\n';
    pos += 1;

    let req_chan = syscalls::chan_open(REQ_CHANNEL.as_ptr());
    let resp_chan = syscalls::chan_open(RESP_CHANNEL.as_ptr());
    if req_chan < 0 || resp_chan < 0 {
        let m = b"init: cannot connect to initd (is PID 1 running?)\n";
        syscalls::write(1, m.as_ptr(), m.len());
        syscalls::exit(1);
    }

    let sent = syscalls::chan_send(req_chan as u32, req.as_ptr(), pos as u32);
    if sent < 0 {
        let m = b"init: failed to send request\n";
        syscalls::write(1, m.as_ptr(), m.len());
        syscalls::exit(1);
    }

    let mut resp_buf = [0u8; MAX_MSG_LEN];
    let n = syscalls::chan_recv(
        resp_chan as u32,
        resp_buf.as_mut_ptr(),
        resp_buf.len() as u32,
    );
    if n > 0 {
        syscalls::write(1, resp_buf.as_ptr(), n as usize);
        if n as usize <= resp_buf.len() && resp_buf[n as usize - 1] != b'\n' {
            syscalls::write(1, b"\n".as_ptr(), 1);
        }
    } else {
        let m = b"init: no response from initd\n";
        syscalls::write(1, m.as_ptr(), m.len());
    }

    syscalls::exit(0);
}

unsafe fn read_argv(argv: *const u64, i: usize) -> &'static [u8] {
    if argv.is_null() {
        return &[];
    }
    let p = *argv.add(i);
    if p == 0 {
        return &[];
    }
    let p = p as *const u8;
    let mut len = 0usize;
    while *p.add(len) != 0 && len < 256 {
        len += 1;
    }
    core::slice::from_raw_parts(p, len)
}
