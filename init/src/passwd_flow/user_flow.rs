//! Ring-2 (self-service) password change flow: a regular user changes
//! their own password after re-authenticating with the current one.

use onyx_init::{syscalls, term};
use term::read_secret_line;

use crate::auth;
use crate::passwd_flow::errno::write_errno;

pub unsafe fn do_user_passwd() { unsafe {
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
            syscalls::write(
                1,
                b"passwd: cannot open /etc/passwd: ".as_ptr(),
                b"passwd: cannot open /etc/passwd: ".len(),
            );
            write_errno(errno);
            syscalls::exit(1);
        }
    };
    let idx = match auth::find_user_by_uid(&users, nusers, uid) {
        Some(i) => i,
        None => {
            syscalls::write(
                1,
                b"passwd: cannot identify current user\n".as_ptr(),
                b"passwd: cannot identify current user\n".len(),
            );
            syscalls::exit(1);
        }
    };
    let me_name = &users[idx].name[..];
    let mut me_len = 0usize;
    while me_len < me_name.len() && me_name[me_len] != 0 {
        me_len += 1;
    }
    let me = &me_name[..me_len];

    syscalls::write(
        1,
        b"Changing password for ".as_ptr(),
        b"Changing password for ".len(),
    );
    syscalls::write(1, me.as_ptr(), me.len());
    syscalls::write(1, b".\n".as_ptr(), b".\n".len());

    let mut old_pass = [0u8; 64];
    syscalls::write(
        1,
        b"Current password: ".as_ptr(),
        b"Current password: ".len(),
    );
    read_secret_line(&mut old_pass);

    if !auth::verify_shadow_password(me, &old_pass) {
        syscalls::write(
            1,
            b"passwd: Authentication failure\n".as_ptr(),
            b"passwd: Authentication failure\n".len(),
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

    match auth::update_shadow_password(me, n1) {
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
