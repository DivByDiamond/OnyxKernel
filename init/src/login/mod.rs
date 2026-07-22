#![no_std]
#![no_main]
#![allow(unsafe_op_in_unsafe_fn, non_snake_case, clippy::missing_safety_doc)]

mod auth;
mod backoff;
mod seed;
mod syscalls;

use core::arch::asm;

const TIOCSRAW: u64 = 0x5421;
const TIOCRRAW: u64 = 0x5422;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _start() -> ! {
    syscalls::write(1, b"\nOnyxOS Login\n".as_ptr(), 14);

    let mut users = [auth::PasswdEntry {
        name: [0; 32],
        uid: 0,
        gid: 0,
        home: [0; 64],
        shell: [0; 32],
    }; auth::MAX_USERS];

    let nusers = auth::read_passwd(&mut users).unwrap_or(0);

    if nusers == 0 {
        syscalls::write(1, b"[login] no users found - auto-login as root\n".as_ptr(), 43);
        seed::seed_root_account();
        let shell = b"/bin/osh\0";
        syscalls::write(1, b"[login] launching /bin/osh (root, ring 1)\n".as_ptr(), 41);
        syscalls::exec(shell.as_ptr(), core::ptr::null());
        syscalls::write(1, b"login: exec failed\n".as_ptr(), 19);
        syscalls::exit(1);
    }

    let mut fails: u32 = 0;
    loop {
        syscalls::write(1, b"\nUsers:\n".as_ptr(), 8);
        for u in users[..nusers].iter() {
            let mut nl = 0;
            while nl < u.name.len() && u.name[nl] != 0 {
                nl += 1;
            }
            if nl > 0 {
                syscalls::write(1, b"  ".as_ptr(), 2);
                syscalls::write(1, u.name.as_ptr(), nl);
                syscalls::write(1, b"\n".as_ptr(), 1);
            }
        }
        syscalls::write(1, b"\n".as_ptr(), 1);

        syscalls::write(1, b"login: ".as_ptr(), 7);
        let mut user_buf = [0u8; 64];
        let n = syscalls::read(0, user_buf.as_mut_ptr(), user_buf.len() as u64);
        if n <= 0 {
            continue;
        }
        let n = n as usize;
        let n = if n > 0 && user_buf[n - 1] == b'\n' { n - 1 } else { n };
        let username = &user_buf[..n];

        if username.is_empty() {
            syscalls::write(1, b"Login incorrect\n\n".as_ptr(), 17);
            continue;
        }

        let user_idx = match auth::find_user(&users, nusers, username) {
            Some(i) => i,
            None => {
                syscalls::write(1, b"Login incorrect\n\n".as_ptr(), 17);
                backoff::backoff_sleep(fails);
                fails = fails.saturating_add(1);
                continue;
            }
        };

        syscalls::write(1, b"password: ".as_ptr(), 10);
        let _ = syscalls::ioctl(0, TIOCSRAW, 0);
        let mut pass_buf = [0u8; 64];
        let pn = syscalls::read(0, pass_buf.as_mut_ptr(), pass_buf.len() as u64);
        let _ = syscalls::ioctl(0, TIOCRRAW, 0);
        syscalls::write(1, b"\n".as_ptr(), 1);

        if pn <= 0 {
            backoff::backoff_sleep(fails);
            fails = fails.saturating_add(1);
            continue;
        }
        let pn = pn as usize;
        let pn = if pn > 0 && pass_buf[pn - 1] == b'\n' { pn - 1 } else { pn };
        let password = &pass_buf[..pn];

        if !auth::verify_shadow_password(username, password) {
            syscalls::write(1, b"Login incorrect\n\n".as_ptr(), 17);
            backoff::backoff_sleep(fails);
            fails = fails.saturating_add(1);
            continue;
        }

        fails = 0;

        let is_root = users[user_idx].uid == 0;
        if is_root {
            syscalls::write(1, b"Login OK (root, ring 1)\n".as_ptr(), 24);
        } else {
            syscalls::write(1, b"Login OK (user, ring 2)\n".as_ptr(), 24);
            syscalls::dropping(2);
        }

        let mut shell_path = [0u8; 32];
        let mut shell_len = 0usize;
        let stored = &users[user_idx].shell;
        while shell_len < stored.len() && stored[shell_len] != 0 {
            shell_path[shell_len] = stored[shell_len];
            shell_len += 1;
        }
        if shell_len == 0 {
            let fallback = b"/bin/osh";
            shell_path[..fallback.len()].copy_from_slice(fallback);
            shell_len = fallback.len();
        }
        if shell_len < shell_path.len() {
            shell_path[shell_len] = 0;
        }
        syscalls::exec(shell_path.as_ptr(), core::ptr::null());
        syscalls::write(1, b"login: exec failed\n".as_ptr(), 19);
    }
}
