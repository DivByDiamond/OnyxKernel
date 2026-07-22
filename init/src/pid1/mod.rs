use crate::boottest;
use crate::syscalls;
use crate::util::write_dec;

pub(crate) mod exec;
mod hdlr;

pub(crate) const LOGIN_PATH: &[u8] = b"/bin/login\0";
pub(crate) const REQ_CHANNEL: &[u8] = b"initd_req\0";
pub(crate) const RESP_CHANNEL: &[u8] = b"initd_resp\0";
pub(crate) const MAX_SERVICES: usize = 16;
pub(crate) const MAX_NAME_LEN: usize = 48;
pub(crate) const MAX_MSG_LEN: usize = 256;
pub(crate) const MAX_PATH_LEN: usize = 96;
pub(crate) const WNOHANG: u32 = 1;

#[derive(Copy, Clone)]
pub(crate) struct ServiceEntry {
    pub(crate) name: [u8; MAX_NAME_LEN],
    pub(crate) name_len: usize,
    pub(crate) pid: u32,
    pub(crate) enabled: bool,
    pub(crate) running: bool,
}

pub(crate) static mut SERVICES: [ServiceEntry; MAX_SERVICES] = [ServiceEntry {
    name: [0; MAX_NAME_LEN],
    name_len: 0,
    pid: 0,
    enabled: false,
    running: false,
}; MAX_SERVICES];

pub(crate) static mut NUM_SERVICES: usize = 0;

pub(crate) unsafe fn pid1_main() -> ! {
    let etc_init = b"/etc/init\0";
    let _ = syscalls::mkdir(etc_init.as_ptr());

    let req_chan = syscalls::chan_create_named(REQ_CHANNEL.as_ptr());
    let resp_chan = syscalls::chan_create_named(RESP_CHANNEL.as_ptr());
    if req_chan < 0 || resp_chan < 0 {
        let m = b"[init:WARN] could not create IPC channels - service control disabled\n";
        syscalls::write(1, m.as_ptr(), m.len());
    } else {
        let m = b"[init] IPC channels initd_req / initd_resp ready\n";
        syscalls::write(1, m.as_ptr(), m.len());
    }

    exec::scan_and_start_services();
    boottest::devfs_boot_test();

    let m = b"[init] launching /bin/login\n";
    syscalls::write(1, m.as_ptr(), m.len());
    let _login_pid = syscalls::spawn(LOGIN_PATH.as_ptr(), core::ptr::null(), 1);

    if req_chan >= 0 && resp_chan >= 0 {
        service_loop(req_chan as u32, resp_chan as u32);
    } else {
        reaper_only_loop();
    }
}

unsafe fn service_loop(req_chan: u32, resp_chan: u32) -> ! {
    let mut req_buf = [0u8; MAX_MSG_LEN];
    let mut resp_buf = [0u8; MAX_MSG_LEN + 32];

    loop {
        reap_children();
        let n = syscalls::chan_recv(req_chan, req_buf.as_mut_ptr(), req_buf.len() as u32);
        if n <= 0 {
            syscalls::yield_cpu();
            continue;
        }
        let n = n as usize;
        let resp_len = hdlr::handle_request(&req_buf[..n], &mut resp_buf);
        let _ = syscalls::chan_send(resp_chan, resp_buf.as_ptr(), resp_len as u32);
    }
}

unsafe fn reaper_only_loop() -> ! {
    loop {
        reap_children();
        syscalls::yield_cpu();
    }
}

unsafe fn reap_children() {
    loop {
        let mut status: i32 = 0;
        let pid = syscalls::waitpid(u32::MAX as u64, &mut status, WNOHANG);
        if pid <= 0 {
            break;
        }
        if let Some(idx) = exec::find_service_by_pid(pid as u32) {
            SERVICES[idx].running = false;
            SERVICES[idx].pid = 0;
            let name = exec::service_name(idx);
            exec::write_state_file(name, b"crashed");
            let m = b"[init] service ";
            syscalls::write(1, m.as_ptr(), m.len());
            syscalls::write(1, name.as_ptr(), name.len());
            let m = b" exited (code=";
            syscalls::write(1, m.as_ptr(), m.len());
            write_dec(status as i64);
            let m = b")\n";
            syscalls::write(1, m.as_ptr(), m.len());
        }
    }
}
