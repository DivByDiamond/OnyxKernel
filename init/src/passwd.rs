#![no_std]
#![no_main]
// TODO(2026-08-27): bin-root allow — raw syscall asm runs inside `unsafe fn`
// wrappers (no_std, per-bin compile); re-evaluate on toolchain/edition bump.
#![allow(unsafe_op_in_unsafe_fn)]

use core::arch::asm;

mod auth;
mod syscalls;
mod term;

// Re-exported for readability at the call sites below.
use term::read_secret_line;

#[unsafe(no_mangle)]
/// # Safety
///
/// Process entry point: called directly by the kernel from the ELF entry
/// address; the stack is freshly initialized per the RISC-V calling convention.
pub unsafe extern "C" fn _start() -> ! {
    let ring = syscalls::getring();

    if ring == 2 {
        do_user_passwd();
    } else {
        do_root_passwd();
    }

    syscalls::exit(0);
}

/// Writes a negative kernel errno as a decimal string followed by a
/// newline, e.g. `-1\n`. Used to surface real syscall failures instead of
/// silently discarding them as a generic "unknown" condition.
unsafe fn write_errno(errno: i64) {
    let mut buf = [0u8; 21];
    let mut i = buf.len();
    let mut n = if errno < 0 { -errno } else { errno } as u64;
    loop {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    if errno < 0 {
        i -= 1;
        buf[i] = b'-';
    }
    syscalls::write(1, buf[i..].as_ptr(), buf.len() - i);
    syscalls::write(1, b"\n".as_ptr(), 1);
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
    let nusers = match auth::read_passwd(&mut users) {
        Ok(n) => n,
        Err(errno) => {
            syscalls::write(1, b"passwd: cannot open /etc/passwd: ".as_ptr(), 34);
            write_errno(errno);
            syscalls::exit(1);
        }
    };
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
    read_secret_line(&mut old_pass);

    if !auth::verify_shadow_password(me, &old_pass) {
        syscalls::write(1, b"passwd: Authentication failure\n".as_ptr(), 33);
        syscalls::exit(1);
    }

    let mut new_pass = [0u8; 64];
    let mut confirm = [0u8; 64];
    syscalls::write(1, b"New password: ".as_ptr(), 14);
    let n1 = read_secret_line(&mut new_pass);
    syscalls::write(1, b"Retype new password: ".as_ptr(), 22);
    let n2 = read_secret_line(&mut confirm);

    if n1.is_empty() || n1.len() != n2.len() || !auth::const_time_eq(n1, n2) {
        syscalls::write(1, b"passwd: Passwords do not match\n".as_ptr(), 34);
        syscalls::exit(1);
    }

    match auth::update_shadow_password(me, n1) {
        Ok(()) => {
            syscalls::write(1, b"passwd: password updated\n".as_ptr(), 25);
        }
        Err(errno) => {
            syscalls::write(1, b"passwd: Failed to update password: ".as_ptr(), 36);
            write_errno(errno);
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
    let n1 = read_secret_line(&mut new_pass);
    syscalls::write(1, b"Retype new password: ".as_ptr(), 22);
    let n2 = read_secret_line(&mut confirm);

    if n1.is_empty() || n1.len() != n2.len() || !auth::const_time_eq(n1, n2) {
        syscalls::write(1, b"passwd: Passwords do not match\n".as_ptr(), 34);
        syscalls::exit(1);
    }

    match auth::update_shadow_password(uname, n1) {
        Ok(()) => {
            syscalls::write(1, b"passwd: password updated\n".as_ptr(), 25);
        }
        Err(errno) => {
            syscalls::write(1, b"passwd: Failed to update password: ".as_ptr(), 36);
            write_errno(errno);
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

// The old local read_password() (single raw read, audit fix 🟡 #2) was
// removed: in kernel raw mode one read() returns after ANY keypress, so a
// one-char "password" was submitted on every key (see term.rs header for
// the full post-mortem). read_secret_line() loops until Enter instead.

/// Audit fix (🔴 #6): mirror the validation used by `useradd` so an
/// operator can't inject a colon or newline into /etc/shadow via the
/// root-passwd path. Rejects anything outside [A-Za-z0-9-_.].
///
/// "root" is deliberately ALLOWED here (fix 2026-09-04): the ring-2 branch
/// only runs for non-root callers, so rejecting "root" left the ring-1
/// operator with NO way to reset root's own password — exactly the
/// recovery path needed after a botched `passwd` run locked an account.
/// Root is already omnipotent (it can rewrite /etc/shadow directly), so
/// this grants nothing new; the charset check still blocks injection.
fn valid_username(u: &[u8]) -> bool {
    !u.is_empty()
        && u.len() <= 31
        && u.iter()
            .all(|&b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {
        unsafe {
            asm!("wfi");
        }
    }
}
