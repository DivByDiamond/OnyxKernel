use super::exec::{
    self, build_enabled_path, find_service_by_name, refresh_service, service_name,
    try_spawn_service, write_state_file,
};
use super::{MAX_NAME_LEN, MAX_PATH_LEN, NUM_SERVICES, SERVICES};
use crate::syscalls;
use crate::util::{dec_slice, format_dec, split_first_token, trim_space, write_response};
pub(super) unsafe fn handle_request(req: &[u8], resp: &mut [u8]) -> usize {
    let mut req = req;
    while !req.is_empty() && (req[req.len() - 1] == b'\n' || req[req.len() - 1] == b'\r') {
        req = &req[..req.len() - 1];
    }
    if req.is_empty() {
        return write_response(resp, b"ERROR empty_request");
    }
    let (cmd, rest) = split_first_token(req);
    let arg = trim_space(rest);
    match cmd {
        b"status" => handle_status(arg, resp),
        b"start" => handle_start(arg, resp),
        b"stop" => handle_stop(arg, resp),
        b"enable" => handle_enable(arg, resp),
        b"disable" => handle_disable(arg, resp),
        b"list" => handle_list(resp),
        _ => write_response(resp, b"ERROR unknown_command"),
    }
}
unsafe fn handle_status(arg: &[u8], resp: &mut [u8]) -> usize {
    if arg.is_empty() {
        return write_response(resp, b"ERROR missing_service_name");
    }
    let idx = match find_service_by_name(arg) {
        Some(i) => i,
        None => return write_response(resp, b"ERROR not_found"),
    };
    refresh_service(idx);
    let pid = SERVICES[idx].pid;
    let running = SERVICES[idx].running;
    let enabled = SERVICES[idx].enabled;
    let state: &[u8] = if running {
        b"running"
    } else if enabled {
        b"stopped"
    } else {
        b"disabled"
    };
    let mut buf = [0u8; 64];
    buf[..3].copy_from_slice(b"OK ");
    let mut pos = 3;
    buf[pos..pos + state.len()].copy_from_slice(state);
    pos += state.len();
    if running && pid > 0 {
        let tag = b" pid=";
        buf[pos..pos + tag.len()].copy_from_slice(tag);
        pos += tag.len();
        let s = format_dec(pid as i64);
        let d = dec_slice(&s);
        let n = d.len().min(buf.len() - pos);
        buf[pos..pos + n].copy_from_slice(&d[..n]);
        pos += n;
    }
    let copy = pos.min(resp.len());
    resp[..copy].copy_from_slice(&buf[..copy]);
    copy
}
unsafe fn handle_start(arg: &[u8], resp: &mut [u8]) -> usize {
    if arg.is_empty() {
        return write_response(resp, b"ERROR missing_service_name");
    }
    let idx = match find_service_by_name(arg) {
        Some(i) => i,
        None => return write_response(resp, b"ERROR not_found"),
    };
    refresh_service(idx);
    if SERVICES[idx].running {
        return write_response(resp, b"OK already_running");
    }
    if try_spawn_service(idx) {
        let pid = SERVICES[idx].pid;
        let mut buf = [0u8; 32];
        buf[..3].copy_from_slice(b"OK ");
        let mut pos = 3;
        let s = format_dec(pid as i64);
        let d = dec_slice(&s);
        let n = d.len().min(buf.len() - pos);
        buf[pos..pos + n].copy_from_slice(&d[..n]);
        pos += n;
        let copy = pos.min(resp.len());
        resp[..copy].copy_from_slice(&buf[..copy]);
        copy
    } else {
        write_response(resp, b"ERROR spawn_failed")
    }
}
unsafe fn handle_stop(arg: &[u8], resp: &mut [u8]) -> usize {
    if arg.is_empty() {
        return write_response(resp, b"ERROR missing_service_name");
    }
    let idx = match find_service_by_name(arg) {
        Some(i) => i,
        None => return write_response(resp, b"ERROR not_found"),
    };
    refresh_service(idx);
    if !SERVICES[idx].running || SERVICES[idx].pid == 0 {
        return write_response(resp, b"OK not_running");
    }
    let r = syscalls::kill(SERVICES[idx].pid, 9);
    if r < 0 {
        return write_response(resp, b"ERROR kill_failed");
    }
    SERVICES[idx].running = false;
    SERVICES[idx].pid = 0;
    let name = service_name(idx);
    write_state_file(name, b"stopped");
    write_response(resp, b"OK stopped")
}
unsafe fn handle_enable(arg: &[u8], resp: &mut [u8]) -> usize {
    if arg.is_empty() {
        return write_response(resp, b"ERROR missing_service_name");
    }
    let idx = match find_service_by_name(arg) {
        Some(i) => i,
        None => return write_response(resp, b"ERROR not_found"),
    };
    let mut path = [0u8; MAX_PATH_LEN];
    let _ = build_enabled_path(arg, &mut path);
    let fd = syscalls::create(path.as_ptr(), 0, 0);
    if fd >= 0 {
        let _ = syscalls::close(fd as u64);
    }
    SERVICES[idx].enabled = true;
    write_response(resp, b"OK enabled")
}
unsafe fn handle_disable(arg: &[u8], resp: &mut [u8]) -> usize {
    if arg.is_empty() {
        return write_response(resp, b"ERROR missing_service_name");
    }
    let idx = match find_service_by_name(arg) {
        Some(i) => i,
        None => return write_response(resp, b"ERROR not_found"),
    };
    refresh_service(idx);
    let mut path = [0u8; MAX_PATH_LEN];
    let _ = build_enabled_path(arg, &mut path);
    let _ = syscalls::unlink(path.as_ptr());
    SERVICES[idx].enabled = false;
    if SERVICES[idx].running && SERVICES[idx].pid != 0 {
        let _ = syscalls::kill(SERVICES[idx].pid, 9);
        SERVICES[idx].running = false;
        SERVICES[idx].pid = 0;
        let name = service_name(idx);
        write_state_file(name, b"disabled");
        return write_response(resp, b"OK disabled_and_stopped");
    }
    let name = service_name(idx);
    write_state_file(name, b"disabled");
    write_response(resp, b"OK disabled")
}
unsafe fn handle_list(resp: &mut [u8]) -> usize {
    let mut pos = 0;
    let header = b"OK services:\n";
    let copy = header.len().min(resp.len() - pos);
    resp[pos..pos + copy].copy_from_slice(&header[..copy]);
    pos += copy;
    for svc in SERVICES[..NUM_SERVICES].iter() {
        if pos >= resp.len() {
            break;
        }
        let name = &svc.name[..svc.name_len];
        let state: &[u8] = if svc.running {
            b"running"
        } else if svc.enabled {
            b"stopped"
        } else {
            b"disabled"
        };
        let mut line = [0u8; MAX_NAME_LEN + 16];
        let mut lp = 0;
        line[lp] = b' ';
        lp += 1;
        line[lp] = b' ';
        lp += 1;
        let nl = name.len().min(line.len() - lp - 1);
        line[lp..lp + nl].copy_from_slice(&name[..nl]);
        lp += nl;
        line[lp] = b' ';
        lp += 1;
        let sl = state.len().min(line.len() - lp - 1);
        line[lp..lp + sl].copy_from_slice(&state[..sl]);
        lp += sl;
        if lp < line.len() {
            line[lp] = b'\n';
            lp += 1;
        }
        let copy = lp.min(resp.len() - pos);
        resp[pos..pos + copy].copy_from_slice(&line[..copy]);
        pos += copy;
    }
    pos
}
