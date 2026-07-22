#![no_std]
#![no_main]
#![allow(unsafe_op_in_unsafe_fn, non_snake_case, clippy::missing_safety_doc)]

use core::arch::asm;

mod auth;
mod syscalls;

/// ioctl numbers — match the kernel's `fs_sys3/extra.rs` definitions.
const TIOCSRAW: u64 = 0x5421;
const TIOCRRAW: u64 = 0x5422;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _start() -> ! {
    let ring = syscalls::getring();

    if ring == 2 {
        do_user_passwd();
    } else {
        do_root_passwd();
    }

    syscalls::exit(0);
}

unsafe fn do_user_passwd() {
    // Audit fix (🔴 #5): the previous code unconditionally verified
    // and changed the password for the hardcoded user `"root"`, even
    // though this branch runs for ring-2 (non-root) callers. That
    // meant (a) a regular user who knew root's password could change
    // it, and (b) a regular user could NOT change their OWN password.
    // We now resolve the caller's uid via getuid() and operate on the
    // matching passwd entry.
    let uid = syscalls::getuid() as u32;

    // Look up the caller's username.
    let mut users = [auth::PasswdEntry {
        name: [0; 32],
        uid: 0,
        gid: 0,
        home: [0; 64],
        shell: [0; 32],
    }; auth::MAX_USERS];
    let nusers = auth::read_passwd(&mut users).unwrap_or(0);
    let idx = match auth::find_user_by_uid(&users, nusers, uid) {
        Some(i) => i,
        None => {
            syscalls::write(1, b"passwd: cannot identify current user\n".as_ptr(), 41);
            syscalls::exit(1);
        }
    };
    let me_name = &users[idx].name[..];
    let mut me_len = 0usize;
    while me_len < me_name.len() && me_name[me_len] != 0 {
        me_len += 1;
    }
    let me = &me_name[..me_len];

    syscalls::write(1, b"Changing password for ".as_ptr(), 22);
    syscalls::write(1, me.as_ptr(), me.len());
    syscalls::write(1, b".\n".as_ptr(), 2);

    let mut old_pass = [0u8; 64];
    syscalls::write(1, b"Current password: ".as_ptr(), 18);
    read_password(&mut old_pass);

    if !auth::verify_shadow_password(me, old_pass) {
        syscalls::write(1, b"passwd: Authentication failure\n".as_ptr(), 33);
        syscalls::exit(1);
    }

    let mut new_pass = [0u8; 64];
    let mut confirm = [0u8; 64];
    syscalls::write(1, b"New password: ".as_ptr(), 14);
    let n1 = read_password(&mut new_pass);
    syscalls::write(1, b"Retype new password: ".as_ptr(), 22);
    let n2 = read_password(&mut confirm);
    syscalls::write(1, b"\n".as_ptr(), 1);

    if n1.is_empty() || n1.len() != n2.len() || !auth::const_time_eq(n1, n2) {
        syscalls::write(1, b"passwd: Passwords do not match\n".as_ptr(), 34);
        syscalls::exit(1);
    }

    match auth::update_shadow_password(me, n1) {
        Ok(()) => {
            syscalls::write(1, b"passwd: password updated\n".as_ptr(), 25);
        }
        Err(_) => {
            syscalls::write(1, b"passwd: Failed to update password\n".as_ptr(), 34);
        }
    }
}

unsafe fn do_root_passwd() {
    let mut username = [0u8; 32];
    syscalls::write(1, b"Username: ".as_ptr(), 10);
    let uname = read_line(&mut username);
    if uname.is_empty() {
        syscalls::write(1, b"passwd: no username\n".as_ptr(), 21);
        syscalls::exit(1);
    }

    // Audit fix (🔴 #6): validate the username so a root operator can't
    // inject a colon or newline into /etc/passwd via this path either.
    if !valid_username(uname) {
        syscalls::write(1, b"passwd: invalid username\n".as_ptr(), 26);
        syscalls::exit(1);
    }

    let mut new_pass = [0u8; 64];
    let mut confirm = [0u8; 64];
    syscalls::write(1, b"New password: ".as_ptr(), 14);
    let n1 = read_password(&mut new_pass);
    syscalls::write(1, b"Retype new password: ".as_ptr(), 22);
    let n2 = read_password(&mut confirm);
    syscalls::write(1, b"\n".as_ptr(), 1);

    if n1.is_empty() || n1.len() != n2.len() || !auth::const_time_eq(n1, n2) {
        syscalls::write(1, b"passwd: Passwords do not match\n".as_ptr(), 34);
        syscalls::exit(1);
    }

    match auth::update_shadow_password(uname, n1) {
        Ok(()) => {
            syscalls::write(1, b"passwd: password updated\n".as_ptr(), 25);
        }
        Err(_) => {
            syscalls::write(1, b"passwd: Failed to update password\n".as_ptr(), 34);
        }
    }
}

unsafe fn read_line(buf: &mut [u8]) -> &[u8] {
    let n = syscalls::read(0, buf.as_mut_ptr(), (buf.len() - 1) as u64);
    if n <= 0 {
        return &[];
    }
    let mut n = n as usize;
    while n > 0 && (buf[n - 1] == b'\n' || buf[n - 1] == b'\r' || buf[n - 1] == 0) {
        n -= 1;
    }
    &buf[..n]
}

/// Read a password from stdin with the terminal temporarily switched
/// to raw mode so the cooked-mode echo doesn't leak the password onto
/// the screen. (Audit fix 🟡 #2.) Returns a slice into `buf`.
unsafe fn read_password(buf: &mut [u8]) -> &[u8] {
    let _ = syscalls::ioctl(0, TIOCSRAW, 0);
    let res = read_line(buf);
    let _ = syscalls::ioctl(0, TIOCRRAW, 0);
    res
}

/// Audit fix (🔴 #6): mirror the validation used by `useradd` so an
/// operator can't inject a colon or newline into /etc/shadow via the
/// root-passwd path. Rejects anything outside [A-Za-z0-9-_.] and the
/// literal name "root" (root's password is managed via the ring-2
/// branch which already targets the current uid).
fn valid_username(u: &[u8]) -> bool {
    !u.is_empty()
        && u.len() <= 31
        && u.iter()
            .all(|&b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
        && u != b"root"
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {
        unsafe {
            asm!("wfi");
        }
    }
}
