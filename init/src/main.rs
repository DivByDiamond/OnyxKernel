//! OnyxOS PID 1 init — service manager ("OnyxInit").
//!
//! Two operating modes, selected by `argc`:
//!
//! 1. **PID 1 mode** (`argc == 0`, launched by the kernel at boot):
//!    - Ensure `/etc/init/` exists.
//!    - Create two named IPC channels: `initd_req` (client → init) and
//!      `initd_resp` (init → client).
//!    - Scan `/service/` for any file (any extension). For each entry:
//!        * If `/etc/init/<name>.enabled` exists → spawn it in ring 1,
//!          remember `(name, pid)` in the in-memory service table, and
//!          write `/etc/init/<name>.state = running`.
//!        * Otherwise write `<name>.state = disabled`.
//!    - Spawn `/bin/login` (replaces the original behaviour) so a user
//!      can log in.
//!    - Enter the service-control loop: block on `initd_req`, dispatch
//!      the request (`status` / `start` / `stop` / `enable` / `disable`
//!      / `list`), send the response on `initd_resp`, then reap any
//!      exited children with non-blocking `waitpid(WNOHANG)` so that
//!      crashed services are marked `crashed` in their `.state` file.
//!
//! 2. **Control mode** (`argc > 0`, exec'd by a user from the shell):
//!    - `argv[0]` is the binary path (ignored).
//!    - `argv[1]` is the command (`status` / `start` / `stop` /
//!      `enable` / `disable` / `list`).
//!    - `argv[2]` (optional) is the service name.
//!    - Open `initd_req` + `initd_resp`, send `<CMD> <ARG>\n`, block on
//!      the response channel, print it, exit.
//!
//! Because `/bin/init` is built with `--ring=1`, only root (whose login
//! session stays in ring 1) can exec it. Regular users stay in ring 2
//! and the kernel's exec ACL will reject the call with `EPERM` — which
//! is exactly the desired "only root can manage services" behaviour.

#![no_std]
#![no_main]
#![warn(clippy::all)]
#![allow(clippy::missing_safety_doc, unsafe_op_in_unsafe_fn, non_snake_case)]

use core::arch::asm;

mod syscalls;

// ── Constants ────────────────────────────────────────────────────────────

const BANNER: &[u8] = b"[init] OnyxOS init v0.4 (service manager)\n";

const LOGIN_PATH: &[u8] = b"/bin/login\0";

const REQ_CHANNEL: &[u8] = b"initd_req\0";
const RESP_CHANNEL: &[u8] = b"initd_resp\0";

const MAX_SERVICES: usize = 16;
const MAX_NAME_LEN: usize = 48;
const MAX_MSG_LEN: usize = 256;
const MAX_PATH_LEN: usize = 96;

const WNOHANG: u32 = 1;

// ── In-memory service table (PID 1 only) ─────────────────────────────────

#[derive(Copy, Clone)]
struct ServiceEntry {
    name: [u8; MAX_NAME_LEN],
    name_len: usize,
    pid: u32,
    enabled: bool,
    running: bool,
}

static mut SERVICES: [ServiceEntry; MAX_SERVICES] = [ServiceEntry {
    name: [0; MAX_NAME_LEN],
    name_len: 0,
    pid: 0,
    enabled: false,
    running: false,
}; MAX_SERVICES];

static mut NUM_SERVICES: usize = 0;

// ── Entry point ──────────────────────────────────────────────────────────

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _start(argc: usize, argv: *const u64) -> ! {
    syscalls::write(1, BANNER.as_ptr(), BANNER.len());

    if argc > 0 {
        // Control mode — exec'd by a user with arguments.
        control_main(argc, argv);
    } else {
        // PID 1 mode — launched by the kernel at boot.
        pid1_main();
    }
}

// ── PID 1 mode ───────────────────────────────────────────────────────────

unsafe fn pid1_main() -> ! {
    // Ensure /etc/init/ exists (state files live here). mkdir returns
    // EEXIST (negative errno) if it already exists — that's fine.
    let etc_init = b"/etc/init\0";
    let _ = syscalls::mkdir(etc_init.as_ptr());

    // Create the two named IPC channels. If creation fails (e.g. they
    // already exist from a previous boot — shouldn't happen since PID 1
    // is started once), we log and continue without IPC; service control
    // won't work but services will still start.
    let req_chan = syscalls::chan_create_named(REQ_CHANNEL.as_ptr());
    let resp_chan = syscalls::chan_create_named(RESP_CHANNEL.as_ptr());
    if req_chan < 0 || resp_chan < 0 {
        let m = b"[init:WARN] could not create IPC channels - service control disabled\n";
        syscalls::write(1, m.as_ptr(), m.len());
    } else {
        let m = b"[init] IPC channels initd_req / initd_resp ready\n";
        syscalls::write(1, m.as_ptr(), m.len());
    }

    // Scan /service/ and start enabled services.
    scan_and_start_services();

    // Launch /bin/login so a user can log in.
    let m = b"[init] launching /bin/login\n";
    syscalls::write(1, m.as_ptr(), m.len());
    let _login_pid = syscalls::spawn(LOGIN_PATH.as_ptr(), core::ptr::null(), 1);

    // Service-control + child-reaper loop.
    if req_chan >= 0 && resp_chan >= 0 {
        service_loop(req_chan as u32, resp_chan as u32);
    } else {
        reaper_only_loop();
    }
}

/// Main PID 1 loop: block on the request channel, dispatch, respond,
/// then reap any exited children before blocking again.
unsafe fn service_loop(req_chan: u32, resp_chan: u32) -> ! {
    let mut req_buf = [0u8; MAX_MSG_LEN];
    let mut resp_buf = [0u8; MAX_MSG_LEN + 32];

    loop {
        // Reap any pending exited children before blocking on IPC.
        // This ensures crashed services are marked quickly even if no
        // new requests arrive for a while.
        reap_children();

        // Block until a request arrives.
        let n = syscalls::chan_recv(req_chan, req_buf.as_mut_ptr(), req_buf.len() as u32);
        if n <= 0 {
            // Woken up but no data (race with another reader — but we
            // are the only reader). Yield and retry.
            syscalls::yield_cpu();
            continue;
        }
        let n = n as usize;

        // Dispatch the request and produce a response.
        let resp_len = handle_request(&req_buf[..n], &mut resp_buf);

        // Send the response back to the client.
        let _ = syscalls::chan_send(resp_chan, resp_buf.as_ptr(), resp_len as u32);
    }
}

/// Fallback loop when IPC channels couldn't be created — just reap
/// children forever so exited services don't leak.
unsafe fn reaper_only_loop() -> ! {
    loop {
        reap_children();
        syscalls::yield_cpu();
    }
}

/// Non-blocking reap of all exited children. Updates the service table
/// and `.state` files for any service that has exited.
unsafe fn reap_children() {
    loop {
        let mut status: i32 = 0;
        // waitpid(-1, &status, WNOHANG) — reap any child, don't block.
        let pid = syscalls::waitpid(u32::MAX as u64, &mut status as *mut i32, WNOHANG);
        if pid <= 0 {
            break;
        }
        // Look up which service this PID belongs to.
        if let Some(idx) = find_service_by_pid(pid as u32) {
            SERVICES[idx].running = false;
            SERVICES[idx].pid = 0;
            let name = service_name(idx);
            write_state_file(name, b"crashed");
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

// ── Service discovery & startup ──────────────────────────────────────────

/// Scan `/service/` once and populate the service table. For each entry,
/// check `/etc/init/<name>.enabled` — if present, spawn the binary in
/// ring 1 and record its PID. State files (`<name>.state`) are written
/// for both enabled and disabled services so `init status` works even
/// before the service has been started.
unsafe fn scan_and_start_services() {
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

        // Skip "." and ".." entries — use byte-by-byte comparison
        // because slice == array comparison can be unreliable in some
        // no_std environments.
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

    // Summary line.
    let m = b"[init] service scan complete: ";
    syscalls::write(1, m.as_ptr(), m.len());
    write_dec(NUM_SERVICES as i64);
    let m = b" services discovered\n";
    syscalls::write(1, m.as_ptr(), m.len());
}

/// Attempt to spawn the service at index `idx`. On success, records
/// the PID and writes `.state = running`. On failure, writes
/// `.state = failed`.
unsafe fn try_spawn_service(idx: usize) -> bool {
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

// ── Request dispatch ─────────────────────────────────────────────────────

/// Check if service `idx` has exited since our last update. If so,
/// update the in-memory state and `.state` file. This is called before
/// responding to `status` / `start` / `stop` / `disable` so the user
/// sees accurate state even if no IPC request has triggered the global
/// `reap_children()` pass yet.
unsafe fn refresh_service(idx: usize) {
    if !SERVICES[idx].running || SERVICES[idx].pid == 0 {
        return;
    }
    let mut status: i32 = 0;
    let r = syscalls::waitpid(SERVICES[idx].pid as u64, &mut status as *mut i32, WNOHANG);
    if r > 0 {
        // The service has exited — reap it and update state.
        SERVICES[idx].running = false;
        SERVICES[idx].pid = 0;
        let name = service_name(idx);
        write_state_file(name, b"crashed");
    }
}

/// Parse a request and write a response into `resp`. Returns the
/// response length. Always produces a response (never panics) so the
/// client never blocks forever waiting for a reply.
unsafe fn handle_request(req: &[u8], resp: &mut [u8]) -> usize {
    // Strip trailing newline.
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
        None => {
            // Even unknown services get a response — useful for debugging.
            return write_response(resp, b"ERROR not_found");
        }
    };
    refresh_service(idx);
    let pid = SERVICES[idx].pid;
    let running = SERVICES[idx].running;
    let enabled = SERVICES[idx].enabled;

    // Build "OK <state> pid=<n>" or "OK <state>".
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
    // SIGKILL = 9.
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
    // Create /etc/init/<name>.enabled. The buffer is zero-initialised so
    // the path is already NUL-terminated after the written bytes.
    let mut path = [0u8; MAX_PATH_LEN];
    let _ = build_enabled_path(arg, &mut path);
    let fd = syscalls::create(path.as_ptr(), 0, 0);
    if fd < 0 {
        // Probably already exists — that's fine, treat as success.
    } else {
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
    // Remove /etc/init/<name>.enabled (buffer is zero-initialised →
    // NUL-terminated).
    let mut path = [0u8; MAX_PATH_LEN];
    let _ = build_enabled_path(arg, &mut path);
    let _ = syscalls::unlink(path.as_ptr());
    SERVICES[idx].enabled = false;

    // If the service is currently running, stop it.
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

    for i in 0..NUM_SERVICES {
        if pos >= resp.len() {
            break;
        }
        let name = service_name(i);
        let state: &[u8] = if SERVICES[i].running {
            b"running"
        } else if SERVICES[i].enabled {
            b"stopped"
        } else {
            b"disabled"
        };

        // "  <name> <state>\n"
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

// ── Control mode (exec'd by user with argv) ──────────────────────────────

unsafe fn control_main(argc: usize, argv: *const u64) -> ! {
    // argv[0] = binary path (ignored)
    // argv[1] = command
    // argv[2] = service name (optional — required for all commands except "list")

    if argc < 2 {
        let m = b"Usage: init <status|start|stop|enable|disable|list> [service]\n";
        syscalls::write(1, m.as_ptr(), m.len());
        syscalls::exit(1);
    }

    let cmd = read_argv(argv, 1);
    let arg = if argc >= 3 { read_argv(argv, 2) } else { &[] };

    // Validate command locally before round-tripping to PID 1.
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

    // `list` takes no argument; the others require a service name.
    if cmd != b"list" && arg.is_empty() {
        let m = b"init: missing service name\n";
        syscalls::write(1, m.as_ptr(), m.len());
        syscalls::exit(1);
    }

    // Build request: "<CMD> <ARG>\n" or "<CMD>\n"
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

    // Open the two named channels.
    let req_chan = syscalls::chan_open(REQ_CHANNEL.as_ptr());
    let resp_chan = syscalls::chan_open(RESP_CHANNEL.as_ptr());
    if req_chan < 0 || resp_chan < 0 {
        let m = b"init: cannot connect to initd (is PID 1 running?)\n";
        syscalls::write(1, m.as_ptr(), m.len());
        syscalls::exit(1);
    }

    // Send the request.
    let sent = syscalls::chan_send(req_chan as u32, req.as_ptr(), pos as u32);
    if sent < 0 {
        let m = b"init: failed to send request\n";
        syscalls::write(1, m.as_ptr(), m.len());
        syscalls::exit(1);
    }

    // Block on the response.
    let mut resp_buf = [0u8; MAX_MSG_LEN];
    let n = syscalls::chan_recv(resp_chan as u32, resp_buf.as_mut_ptr(), resp_buf.len() as u32);
    if n > 0 {
        syscalls::write(1, resp_buf.as_ptr(), n as usize);
        // Ensure a trailing newline for nice shell output.
        if n as usize <= resp_buf.len() && resp_buf[n as usize - 1] != b'\n' {
            syscalls::write(1, b"\n".as_ptr(), 1);
        }
    } else {
        let m = b"init: no response from initd\n";
        syscalls::write(1, m.as_ptr(), m.len());
    }

    syscalls::exit(0);
}

// ── Helpers ──────────────────────────────────────────────────────────────

/// Read `argv[i]` from a user-space argv array. Returns an empty slice
/// on out-of-bounds or null pointer.
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

/// Length of a NUL-terminated byte string in `buf`.
fn cstr_len(buf: &[u8]) -> usize {
    buf.iter().position(|&b| b == 0).unwrap_or(buf.len())
}

/// Check if a name is "." or ".." — use byte-by-byte comparison
/// to avoid any slice-vs-array comparison quirks.
fn is_dot_or_dotdot(name: &[u8]) -> bool {
    if name.len() == 1 && name[0] == b'.' {
        return true;
    }
    if name.len() == 2 && name[0] == b'.' && name[1] == b'.' {
        return true;
    }
    false
}

/// Split `s` at the first ASCII space, returning (first_token, rest).
/// `rest` may still contain leading spaces — use `trim_space` to clean.
fn split_first_token(s: &[u8]) -> (&[u8], &[u8]) {
    let mut i = 0;
    while i < s.len() && s[i] != b' ' && s[i] != b'\t' {
        i += 1;
    }
    (&s[..i], &s[i..])
}

/// Strip leading ASCII whitespace.
fn trim_space(s: &[u8]) -> &[u8] {
    let mut i = 0;
    while i < s.len() && (s[i] == b' ' || s[i] == b'\t') {
        i += 1;
    }
    &s[i..]
}

/// Write a NUL-terminated response string into `resp` and return its length.
fn write_response(resp: &mut [u8], s: &[u8]) -> usize {
    let n = s.len().min(resp.len());
    resp[..n].copy_from_slice(s);
    n
}

/// Look up a service by name in the in-memory table.
unsafe fn find_service_by_name(name: &[u8]) -> Option<usize> {
    for i in 0..NUM_SERVICES {
        let sn = service_name(i);
        if sn == name {
            return Some(i);
        }
    }
    None
}

/// Look up a service by its recorded PID.
unsafe fn find_service_by_pid(pid: u32) -> Option<usize> {
    for i in 0..NUM_SERVICES {
        if SERVICES[i].pid == pid && pid != 0 {
            return Some(i);
        }
    }
    None
}

/// Borrow the name of service `idx` as a `&[u8]` (no trailing NULs).
unsafe fn service_name(idx: usize) -> &'static [u8] {
    &SERVICES[idx].name[..SERVICES[idx].name_len]
}

/// Check whether `/etc/init/<name>.enabled` exists.
unsafe fn is_service_enabled(name: &[u8]) -> bool {
    let mut path = [0u8; MAX_PATH_LEN];
    let _ = build_enabled_path(name, &mut path);
    // Use access(path, F_OK=0) — returns 0 if the file exists.
    // Buffer is zero-initialised → already NUL-terminated.
    let r = syscalls::access(path.as_ptr(), 0);
    r == 0
}

/// Build the full path `/service/<name>` into `out`. The result is
/// NUL-terminated by the caller's zero-initialised buffer. Returns the
/// length written (excluding the implicit NUL).
fn build_service_path(name: &[u8], out: &mut [u8]) -> usize {
    // Leave at least 1 byte for the NUL terminator.
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

/// Build the path `/etc/init/<name>.enabled` into `out`. The result is
/// NUL-terminated by the caller's zero-initialised buffer. Returns the
/// length written (excluding the implicit NUL).
fn build_enabled_path(name: &[u8], out: &mut [u8]) -> usize {
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

/// Build the path `/etc/init/<name>.state` into `out`. The result is
/// NUL-terminated by the caller's zero-initialised buffer. Returns the
/// length written (excluding the implicit NUL).
fn build_state_path(name: &[u8], out: &mut [u8]) -> usize {
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

/// Write `<state>` to `/etc/init/<name>.state`. Creates the file if it
/// doesn't exist, truncates if it does. Best-effort — errors are
/// silently ignored.
unsafe fn write_state_file(name: &[u8], state: &[u8]) {
    let mut path = [0u8; MAX_PATH_LEN];
    let _ = build_state_path(name, &mut path);
    // Buffer is zero-initialised → NUL-terminated.
    // `create` truncates an existing file (it's a fresh inode write).
    let fd = syscalls::create(path.as_ptr(), 0, 0);
    if fd < 0 {
        return;
    }
    let _ = syscalls::write_fd(fd as u64, state.as_ptr(), state.len());
    let _ = syscalls::close(fd as u64);
}

/// Format a signed integer as decimal into a fixed 12-byte buffer.
/// Unused leading bytes are NUL. Use `dec_slice` to extract just the
/// meaningful digits (no leading NULs).
fn format_dec(n: i64) -> [u8; 12] {
    let mut buf = [0u8; 12];
    let mut pos = 11;
    if n == 0 {
        buf[10] = b'0';
        return buf;
    }
    let neg = n < 0;
    let mut val = if neg { (-(n as i128)) as u64 } else { n as u64 };
    while val > 0 && pos > 0 {
        pos -= 1;
        buf[pos] = b'0' + (val % 10) as u8;
        val /= 10;
    }
    if neg && pos > 0 {
        pos -= 1;
        buf[pos] = b'-';
    }
    buf
}

/// Given a buffer produced by `format_dec`, return the contiguous slice
/// of meaningful bytes (skipping leading NULs and the trailing NUL at
/// position 11). Never panics.
fn dec_slice(s: &[u8; 12]) -> &[u8] {
    let start = s.iter().position(|&b| b != 0).unwrap_or(s.len());
    // Find the last non-zero byte; its index + 1 is `end`.
    let end = s.iter().rposition(|&b| b != 0).map(|i| i + 1).unwrap_or(0);
    if start < end {
        &s[start..end]
    } else {
        &[]
    }
}

/// Write a decimal integer to stdout (skipping leading NULs).
unsafe fn write_dec(n: i64) {
    let s = format_dec(n);
    let slice = dec_slice(&s);
    if !slice.is_empty() {
        syscalls::write(1, slice.as_ptr(), slice.len());
    }
}

// ── Panic handler ────────────────────────────────────────────────────────

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {
        unsafe {
            asm!("wfi");
        }
    }
}
