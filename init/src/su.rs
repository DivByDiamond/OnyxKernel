#![no_std]
#![no_main]
#![allow(unsafe_op_in_unsafe_fn, non_snake_case, clippy::missing_safety_doc)]

use core::arch::asm;

mod auth;
mod syscalls;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _start(argc: usize, argv: *const u64) -> ! {
    let mut target_user = [0u8; 32];
    let mut target_len = 0usize;

    // Read username from argv or prompt
    if argc > 1 {
        let arg1 = *argv.add(1);
        if arg1 != 0 {
            let p = arg1 as *const u8;
            while *p.add(target_len) != 0 && target_len < 31 {
                target_user[target_len] = *p.add(target_len);
                target_len += 1;
            }
        }
    }

    if target_len == 0 {
        syscalls::write(1, b"su: username: ".as_ptr(), 14);
        let mut buf = [0u8; 64];
        let n = syscalls::read(0, buf.as_mut_ptr(), buf.len() as u64);
        if n <= 0 {
            syscalls::exit(1);
        }
        let mut n = n as usize;
        while n > 0 && (buf[n - 1] == b'\n' || buf[n - 1] == b'\r') {
            n -= 1;
        }
        target_len = n.min(31);
        target_user[..target_len].copy_from_slice(&buf[..target_len]);
    }

    if target_len == 0 {
        syscalls::write(1, b"su: no username\n".as_ptr(), 17);
        syscalls::exit(1);
    }

    let username = &target_user[..target_len];

    // Read /etc/passwd
    let mut users = [auth::PasswdEntry {
        name: [0; 32],
        uid: 0,
        gid: 0,
        home: [0; 64],
        shell: [0; 32],
    }; auth::MAX_USERS];
    let nusers = auth::read_passwd(&mut users).unwrap_or(0);

    let user_idx = match auth::find_user(&users, nusers, username) {
        Some(i) => i,
        None => {
            syscalls::write(1, b"su: unknown user\n".as_ptr(), 18);
            syscalls::exit(1);
        }
    };

    // Prompt for password
    syscalls::write(1, b"Password: ".as_ptr(), 10);
    let mut pass_buf = [0u8; 64];
    let pn = syscalls::read(0, pass_buf.as_mut_ptr(), pass_buf.len() as u64);
    if pn <= 0 {
        syscalls::write(1, b"\nsu: authentication failed\n".as_ptr(), 27);
        syscalls::exit(1);
    }
    let mut pn = pn as usize;
    while pn > 0 && (pass_buf[pn - 1] == b'\n' || pass_buf[pn - 1] == b'\r') {
        pn -= 1;
    }
    let password = &pass_buf[..pn];

    // Verify password via /etc/shadow
    if !auth::verify_shadow_password(username, password) {
        syscalls::write(1, b"\nsu: authentication failed\n".as_ptr(), 27);
        syscalls::exit(1);
    }

    // Set uid/gid
    let target_uid = users[user_idx].uid;
    let target_gid = users[user_idx].gid;
    let r1 = syscalls::setuid(target_uid as u64);
    let r2 = syscalls::setgid(target_gid as u64);
    if r1 < 0 || r2 < 0 {
        syscalls::write(1, b"su: setuid/setgid failed\n".as_ptr(), 27);
        syscalls::exit(1);
    }

    // Drop to ring 2 for non-root
    if target_uid != 0 {
        syscalls::dropping(2);
    }

    // Exec the user's shell
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
    syscalls::write(1, b"su: exec failed\n".as_ptr(), 16);
    syscalls::exit(1);
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {
        unsafe {
            asm!("wfi");
        }
    }
}
