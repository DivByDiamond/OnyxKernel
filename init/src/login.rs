//! OnyxOS /bin/login — authenticate and exec the user's shell.
//!
//! Login flow:
//! 1. Read /etc/passwd to get the list of users.
//! 2. If NO users exist (fresh install) → auto-login as root, no
//!    password. This lets you into an empty system immediately.
//! 3. If users DO exist → print the user list and prompt for a
//!    username + password. Passwords are verified against the
//!    `$5$salt$hash` entries in /etc/shadow via the iterated SHA-256
//!    KDF implemented in `auth::verify_shadow_password`.
//!
//! Privilege handling:
//! - **root** (uid 0) stays in ring 1 (root space). This lets root
//!   run privileged binaries such as `/bin/init` for service
//!   management.
//! - **regular users** drop to ring 2 (user space) via `dropping(2)`.
//!
//! Audit fixes applied:
//! - 🔴 #1: passwords are no longer compared as plaintext; the verify
//!   path uses the iterated SHA-256 KDF in `auth`.
//! - 🔴 #2: fail-closed authentication. If `/etc/shadow` cannot be
//!   opened (deleted, corrupted, permission denied), authentication
//!   always fails — never falls back to "empty password accepted".
//! - 🔴 #11: exponential backoff on failed attempts (0.25 s → 16 s).
//! - 🟡 #2: passwords are read with the terminal in raw mode so the
//!   cooked-mode echo does not leak them onto the screen.

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
        syscalls::write(
            1,
            b"[login] launching /bin/osh (root, ring 1)\n".as_ptr(),
            41,
        );
        syscalls::exec(shell.as_ptr(), core::ptr::null());
        syscalls::write(1, b"login: exec failed\n".as_ptr(), 19);
        syscalls::exit(1);
    }

    // ── Case 2: users exist → normal login prompt ──────────────────
    let mut fails: u32 = 0;
    loop {
        // Print user list so the user knows what to type.
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
                // Audit fix (🔴 #11): even for unknown users we apply
                // backoff — otherwise an attacker could enumerate valid
                // usernames by timing the response.
                backoff_sleep(fails);
                fails = fails.saturating_add(1);
                continue;
            }
        };

        // Read password in raw mode so the cooked-mode echo doesn't
        // leak it onto the screen. (Audit fix 🟡 #2.)
        syscalls::write(1, b"password: ".as_ptr(), 10);
        let _ = syscalls::ioctl(0, TIOCSRAW, 0);
        let mut pass_buf = [0u8; 64];
        let pn = syscalls::read(0, pass_buf.as_mut_ptr(), pass_buf.len() as u64);
        // Restore cooked mode BEFORE printing the newline so the kernel
        // handles the line editing again on the next prompt.
        let _ = syscalls::ioctl(0, TIOCRRAW, 0);
        syscalls::write(1, b"\n".as_ptr(), 1);

        if pn <= 0 {
            backoff_sleep(fails);
            fails = fails.saturating_add(1);
            continue;
        }
        let pn = pn as usize;
        let pn = if pn > 0 && pass_buf[pn - 1] == b'\n' {
            pn - 1
        } else {
            pn
        };
        let password = &pass_buf[..pn];

        // Verify password against /etc/shadow via the iterated SHA-256
        // KDF. `verify_shadow_password` is fail-closed: if /etc/shadow
        // cannot be read, it returns `false` rather than allowing the
        // user in with an empty password. (Audit fixes 🔴 #1 and 🔴 #2.)
        if !auth::verify_shadow_password(username, password) {
            syscalls::write(1, b"Login incorrect\n\n".as_ptr(), 17);
            backoff_sleep(fails);
            fails = fails.saturating_add(1);
            continue;
        }

        // Successful login resets the failure counter.
        fails = 0;

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
/// later modify it. Writes /etc/passwd and /etc/shadow with the
/// password "root" hashed via the iterated SHA-256 KDF.
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

/// Audit fix (🔴 #11): exponential backoff. `fails` is the number of
/// consecutive failed attempts for this session. We delay by
/// `2^min(fails,6) * 250 ms` — i.e. 0.25 s, 0.5 s, 1 s, 2 s, 4 s,
/// 8 s, 16 s, 16 s, … — capped at 16 s. This makes online brute force
/// impractical even in a cloud console while still being tolerable
/// for a real user who typos their password once.
fn backoff_sleep(fails: u32) {
    let exp = fails.min(6);
    let delay_us: u64 = (1u64 << exp) * 250_000;
    // struct timespec { tv_sec, tv_nsec } — both u64 on RV64.
    let ts: [u64; 2] = [delay_us / 1_000_000, (delay_us % 1_000_000) * 1000];
    unsafe {
        syscalls::nanosleep(ts.as_ptr(), core::ptr::null_mut());
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {
        unsafe {
            asm!("wfi");
        }
    }
}
