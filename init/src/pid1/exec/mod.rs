pub(crate) mod ctl;

use super::{MAX_NAME_LEN, MAX_PATH_LEN, NUM_SERVICES, SERVICES, WNOHANG};
use crate::syscalls;
use crate::util::{cstr_len, is_dot_or_dotdot, write_dec};

pub(super) unsafe fn scan_and_start_services() {
    let dir = b"/service\0";
    let mut name_buf = [0u8; 64];

    loop {
        let ret = syscalls::readdir(dir.as_ptr(), name_buf.as_mut_ptr(), name_buf.len() as u64);
        if ret <= 0 {
            break;
        }
        let name_len = cstr_len(&name_buf);
        if name_len == 0 || name_len >= MAX_NAME_LEN {
            continue;
        }

        if is_dot_or_dotdot(&name_buf[..name_len]) {
            continue;
        }

        if NUM_SERVICES >= MAX_SERVICES {
            let m = b"[init:WARN] service table full - skipping ";
            syscalls::write(1, m.as_ptr(), m.len());
            syscalls::write(1, name_buf.as_ptr(), name_len);
            syscalls::write(1, b"\n".as_ptr(), 1);
            break;
        }

        let idx = NUM_SERVICES;
        NUM_SERVICES += 1;
        SERVICES[idx].name = [0; MAX_NAME_LEN];
        SERVICES[idx].name[..name_len].copy_from_slice(&name_buf[..name_len]);
        SERVICES[idx].name_len = name_len;
        SERVICES[idx].pid = 0;
        SERVICES[idx].running = false;

        let enabled = is_service_enabled(&name_buf[..name_len]);
        SERVICES[idx].enabled = enabled;

        if enabled {
            try_spawn_service(idx);
        } else {
            write_state_file(&name_buf[..name_len], b"disabled");
            let m = b"[init] - service ";
            syscalls::write(1, m.as_ptr(), m.len());
            syscalls::write(1, name_buf.as_ptr(), name_len);
            let m = b" (disabled)\n";
            syscalls::write(1, m.as_ptr(), m.len());
        }
    }

    let m = b"[init] service scan complete: ";
    syscalls::write(1, m.as_ptr(), m.len());
    write_dec(NUM_SERVICES as i64);
    let m = b" services discovered\n";
    syscalls::write(1, m.as_ptr(), m.len());
}

pub(super) unsafe fn try_spawn_service(idx: usize) -> bool {
    let name = service_name(idx);
    let mut path = [0u8; MAX_PATH_LEN];
    let _ = build_service_path(name, &mut path);

    let pid = syscalls::spawn(path.as_ptr(), core::ptr::null(), 1);
    if pid > 0 {
        SERVICES[idx].pid = pid as u32;
        SERVICES[idx].running = true;
        write_state_file(name, b"running");

        let m = b"[init] + service ";
        syscalls::write(1, m.as_ptr(), m.len());
        syscalls::write(1, name.as_ptr(), name.len());
        let m = b" pid=";
        syscalls::write(1, m.as_ptr(), m.len());
        write_dec(pid);
        syscalls::write(1, b"\n".as_ptr(), 1);
        true
    } else {
        write_state_file(name, b"failed");
        let m = b"[init:ERR] spawn failed for ";
        syscalls::write(1, m.as_ptr(), m.len());
        syscalls::write(1, name.as_ptr(), name.len());
        syscalls::write(1, b"\n".as_ptr(), 1);
        false
    }
}

pub(super) unsafe fn refresh_service(idx: usize) {
    if !SERVICES[idx].running || SERVICES[idx].pid == 0 {
        return;
    }
    let mut status: i32 = 0;
    let r = syscalls::waitpid(SERVICES[idx].pid as u64, &mut status, WNOHANG);
    if r > 0 {
        SERVICES[idx].running = false;
        SERVICES[idx].pid = 0;
        let name = service_name(idx);
        write_state_file(name, b"crashed");
    }
}

pub(super) unsafe fn find_service_by_name(name: &[u8]) -> Option<usize> {
    (0..NUM_SERVICES).find(|&i| service_name(i) == name)
}

pub(super) unsafe fn find_service_by_pid(pid: u32) -> Option<usize> {
    SERVICES[..NUM_SERVICES]
        .iter()
        .position(|svc| svc.pid == pid && pid != 0)
}

pub(super) unsafe fn service_name(idx: usize) -> &'static [u8] {
    &SERVICES[idx].name[..SERVICES[idx].name_len]
}

pub(super) unsafe fn is_service_enabled(name: &[u8]) -> bool {
    let mut path = [0u8; MAX_PATH_LEN];
    let _ = build_enabled_path(name, &mut path);
    let r = syscalls::access(path.as_ptr(), 0);
    r == 0
}

pub(super) fn build_service_path(name: &[u8], out: &mut [u8]) -> usize {
    if out.len() < 2 {
        return 0;
    }
    let cap = out.len() - 1;
    let prefix = b"/service/";
    let pl = prefix.len().min(cap);
    out[..pl].copy_from_slice(&prefix[..pl]);
    let nl = name.len().min(cap - pl);
    out[pl..pl + nl].copy_from_slice(&name[..nl]);
    pl + nl
}

pub(super) fn build_enabled_path(name: &[u8], out: &mut [u8]) -> usize {
    if out.len() < 2 {
        return 0;
    }
    let cap = out.len() - 1;
    let prefix = b"/etc/init/";
    let pl = prefix.len().min(cap);
    out[..pl].copy_from_slice(&prefix[..pl]);
    let nl = name.len().min(cap - pl);
    out[pl..pl + nl].copy_from_slice(&name[..nl]);
    let suffix = b".enabled";
    let total = pl + nl;
    let sl = suffix.len().min(cap - total);
    out[total..total + sl].copy_from_slice(&suffix[..sl]);
    total + sl
}

pub(super) fn build_state_path(name: &[u8], out: &mut [u8]) -> usize {
    if out.len() < 2 {
        return 0;
    }
    let cap = out.len() - 1;
    let prefix = b"/etc/init/";
    let pl = prefix.len().min(cap);
    out[..pl].copy_from_slice(&prefix[..pl]);
    let nl = name.len().min(cap - pl);
    out[pl..pl + nl].copy_from_slice(&name[..nl]);
    let suffix = b".state";
    let total = pl + nl;
    let sl = suffix.len().min(cap - total);
    out[total..total + sl].copy_from_slice(&suffix[..sl]);
    total + sl
}

pub(super) unsafe fn write_state_file(name: &[u8], state: &[u8]) {
    let mut path = [0u8; MAX_PATH_LEN];
    let _ = build_state_path(name, &mut path);
    let fd = syscalls::create(path.as_ptr(), 0, 0);
    if fd < 0 {
        return;
    }
    let _ = syscalls::write_fd(fd as u64, state.as_ptr(), state.len());
    let _ = syscalls::close(fd as u64);
}
