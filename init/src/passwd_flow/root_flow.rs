//! Ring-1 (root/operator) password-reset flow: an operator picks a
//! username and sets a new password for it directly, without needing the
//! account's current password.

use onyx_init::{syscalls, term};
use term::read_secret_line;

use crate::auth;
use crate::passwd_flow::errno::write_errno;

pub unsafe fn do_root_passwd() { unsafe {
    let mut username = [0u8; 32];
    syscalls::write(1, b"Username: ".as_ptr(), b"Username: ".len());
    let uname = read_line(&mut username);
    if uname.is_empty() {
        syscalls::write(
            1,
            b"passwd: no username\n".as_ptr(),
            b"passwd: no username\n".len(),
        );
        syscalls::exit(1);
    }

    // Audit fix (🔴 #6): validate the username so a root operator can't
    // inject a colon or newline into /etc/passwd via this path either.
    if !valid_username(uname) {
        syscalls::write(
            1,
            b"passwd: invalid username\n".as_ptr(),
            b"passwd: invalid username\n".len(),
        );
        syscalls::exit(1);
    }

    let mut new_pass = [0u8; 64];
    let mut confirm = [0u8; 64];
    syscalls::write(1, b"New password: ".as_ptr(), b"New password: ".len());
    let n1 = read_secret_line(&mut new_pass);
    syscalls::write(
        1,
        b"Retype new password: ".as_ptr(),
        b"Retype new password: ".len(),
    );
    let n2 = read_secret_line(&mut confirm);

    if n1.is_empty() || n1.len() != n2.len() || !auth::const_time_eq(n1, n2) {
        syscalls::write(
            1,
            b"passwd: Passwords do not match\n".as_ptr(),
            b"passwd: Passwords do not match\n".len(),
        );
        syscalls::exit(1);
    }

    match auth::update_shadow_password(uname, n1) {
        Ok(()) => {
            syscalls::write(
                1,
                b"passwd: password updated\n".as_ptr(),
                b"passwd: password updated\n".len(),
            );
        }
        Err(errno) => {
            syscalls::write(
                1,
                b"passwd: Failed to update password: ".as_ptr(),
                b"passwd: Failed to update password: ".len(),
            );
            write_errno(errno);
        }
    }
}}

unsafe fn read_line(buf: &mut [u8]) -> &[u8] { unsafe {
    let n = syscalls::read(0, buf.as_mut_ptr(), (buf.len() - 1) as u64);
    if n <= 0 {
        return &[];
    }
    let mut n = n as usize;
    while n > 0 && (buf[n - 1] == b'\n' || buf[n - 1] == b'\r' || buf[n - 1] == 0) {
        n -= 1;
    }
    &buf[..n]
}}

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
