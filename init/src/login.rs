//! OnyxOS /bin/login — authenticate and exec the user's shell.
//!
//! Login flow:
//! 1. Read /etc/passwd to get the list of users.
//! 2. If NO users exist (fresh install) → auto-login as root, no
//!    password. This lets you into an empty system immediately.
//! 3. If users DO exist → print the user list and prompt for a
//!    username + password. Passwords are compared in plaintext
//!    against /etc/shadow (simpler than SHA-256+salt, and good enough
//!    for a hobby OS until the crypto path is debugged).
//!
//! Privilege handling:
//! - **root** (uid 0) stays in ring 1 (root space). This lets root
//!   run privileged binaries such as `/bin/init` for service
//!   management.
//! - **regular users** drop to ring 2 (user space) via `dropping(2)`.

#![no_std]
#![no_main]
#![allow(unsafe_op_in_unsafe_fn, non_snake_case, clippy::missing_safety_doc)]

use core::arch::asm;

mod auth;
mod syscalls;

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

    // ── Case 1: no users → auto-login as root ──────────────────────
    // Fresh install. Seed a root account and drop straight into the
    // shell so the user can start administering the system.
    if nusers == 0 {
        syscalls::write(
            1,
            b"[login] no users found - auto-login as root\n".as_ptr(),
            43,
        );
        seed_root_account();
        // Exec /bin/osh directly as root (ring 1).
        let shell = b"/bin/osh\0";
        syscalls::write(1, b"[login] launching /bin/osh (root, ring 1)\n".as_ptr(), 41);
        syscalls::exec(shell.as_ptr(), core::ptr::null());
        syscalls::write(1, b"login: exec failed\n".as_ptr(), 19);
        syscalls::exit(1);
    }

    // ── Case 2: users exist → normal login prompt ──────────────────
    loop {
        // Print user list so the user knows what to type.
        syscalls::write(1, b"\nUsers:\n".as_ptr(), 8);
        for i in 0..nusers {
            let mut nl = 0;
            while nl < users[i].name.len() && users[i].name[nl] != 0 {
                nl += 1;
            }
            if nl > 0 {
                syscalls::write(1, b"  ".as_ptr(), 2);
                syscalls::write(1, users[i].name.as_ptr(), nl);
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
        let n = if n > 0 && user_buf[n - 1] == b'\n' {
            n - 1
        } else {
            n
        };
        let username = &user_buf[..n];

        if username.is_empty() {
            syscalls::write(1, b"Login incorrect\n\n".as_ptr(), 17);
            continue;
        }

        // Find user.
        let user_idx = match auth::find_user(&users, nusers, username) {
            Some(i) => i,
            None => {
                syscalls::write(1, b"Login incorrect\n\n".as_ptr(), 17);
                continue;
            }
        };

        // Read password.
        syscalls::write(1, b"password: ".as_ptr(), 10);
        let mut pass_buf = [0u8; 64];
        let pn = syscalls::read(0, pass_buf.as_mut_ptr(), pass_buf.len() as u64);
        if pn <= 0 {
            continue;
        }
        let pn = pn as usize;
        let pn = if pn > 0 && pass_buf[pn - 1] == b'\n' {
            pn - 1
        } else {
            pn
        };
        let password = &pass_buf[..pn];

        // Verify password (plaintext comparison against /etc/shadow).
        if !verify_plaintext_password(username, password) {
            syscalls::write(1, b"Login incorrect\n\n".as_ptr(), 17);
            continue;
        }

        // Success — set ring and exec shell.
        let is_root = users[user_idx].uid == 0;
        if is_root {
            syscalls::write(1, b"Login OK (root, ring 1)\n".as_ptr(), 24);
        } else {
            syscalls::write(1, b"Login OK (user, ring 2)\n".as_ptr(), 24);
            syscalls::dropping(2);
        }

        // Exec the user's shell (default /bin/osh).
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

/// Create a minimal root account so that `useradd` / `passwd` can
/// later modify it. Writes /etc/passwd and /etc/shadow with password
/// "root" in plaintext.
unsafe fn seed_root_account() {
    let home = b"/users/root";
    let shell = b"/bin/osh";
    let _ = auth::update_passwd_entry(b"root", 0, 0, home, shell);
    let _ = auth::update_shadow_password(b"root", b"root");

    // Create /users/root directory.
    let mut mkdir_buf = [0u8; 64];
    mkdir_buf[..11].copy_from_slice(b"/users/root");
    let _ = syscalls::mkdir(mkdir_buf.as_ptr());
}

/// Verify a password against /etc/shadow using PLAINTEXT comparison.
/// The shadow entry format is `<username>:<plaintext_password>`.
/// This is intentionally simple — no SHA-256, no salt, no hex. It's
/// a hobby OS; we'll add proper hashing later once the crypto path
/// is debugged.
unsafe fn verify_plaintext_password(username: &[u8], password: &[u8]) -> bool {
    // Read /etc/shadow.
    let mut path_buf = [0u8; 64];
    let path = b"/etc/shadow";
    path_buf[..path.len()].copy_from_slice(path);
    path_buf[path.len()] = 0;

    let fd = syscalls::open(path_buf.as_ptr(), 0, 0);
    if fd < 0 {
        // No shadow file → if password is empty, allow (for first-boot
        // edge cases). Otherwise reject.
        return password.is_empty();
    }

    let mut buf = [0u8; 4096];
    let mut total = 0usize;
    loop {
        let n = syscalls::read(fd as u64, buf[total..].as_mut_ptr(), (buf.len() - total) as u64);
        if n <= 0 {
            break;
        }
        total += n as usize;
        if total >= buf.len() {
            break;
        }
    }
    syscalls::close(fd as u64);

    // Parse line by line: `username:password`
    let data = &buf[..total];
    let mut pos = 0;
    while pos < data.len() {
        let line_end = data[pos..].iter().position(|&b| b == b'\n').map(|n| pos + n).unwrap_or(data.len());
        let line = &data[pos..line_end];
        pos = line_end + 1;

        let colon = match line.iter().position(|&b| b == b':') {
            Some(n) => n,
            None => continue,
        };
        let name = &line[..colon];
        let stored_pass = &line[colon + 1..];

        if name.len() == username.len() && name == username {
            // Found the user — compare passwords byte by byte.
            if stored_pass.len() != password.len() {
                return false;
            }
            let mut ok = true;
            for i in 0..stored_pass.len() {
                if stored_pass[i] != password[i] {
                    ok = false;
                }
            }
            return ok;
        }
    }
    false
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {
        unsafe {
            asm!("wfi");
        }
    }
}
