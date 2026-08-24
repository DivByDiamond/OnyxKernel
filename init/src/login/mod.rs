#![no_std]
#![no_main]
#![allow(unsafe_op_in_unsafe_fn, non_snake_case, clippy::missing_safety_doc)]

#[path = "../auth/mod.rs"]
mod auth;
#[path = "../syscalls/mod.rs"]
mod syscalls;

mod backoff;
mod seed;

const TIOCSRAW: u64 = 0x5421;
const TIOCRRAW: u64 = 0x5422;

/// Write NUL-terminated concatenation of `parts` into `buf`, returning the
/// length including the terminator. Strings must be NUL-terminated to match
/// the kernel-side char** copy logic (proc::onx::argv).
fn put_env(buf: &mut [u8], parts: &[&[u8]]) -> usize {
    let mut i = 0usize;
    for part in parts {
        for &b in *part {
            if i + 1 >= buf.len() {
                break;
            }
            buf[i] = b;
            i += 1;
        }
    }
    buf[i] = 0;
    i + 1
}

/// Build argv/envp for the shell and exec it via SYS_execve.
///
/// Layout passed as raw user pointers (kernel copies them onto the new
/// image's stack):
///   argv = [ptr "osh", NULL]
///   envp = ["HOME=/users/<name>", "USER=<name>", "SHELL=<shell>", "PATH=/bin", NULL]
fn exec_shell(username: &[u8], shell_path: &[u8]) -> i64 {
    let mut arg0 = [0u8; 8];
    let name = b"osh";
    arg0[..name.len()].copy_from_slice(name);

    let mut env_home = [0u8; 80];
    put_env(&mut env_home, &[b"HOME=/users/", username]);
    let mut env_user = [0u8; 48];
    put_env(&mut env_user, &[b"USER=", username]);
    let mut env_shell = [0u8; 48];
    put_env(&mut env_shell, &[b"SHELL=", shell_path]);

    // Image layout only ships /bin (no /sbin), so PATH=/bin for all users.
    let env_path = b"PATH=/bin\0";

    let argv = [arg0.as_ptr() as u64, 0u64];
    let envp = [
        env_home.as_ptr() as u64,
        env_user.as_ptr() as u64,
        env_shell.as_ptr() as u64,
        env_path.as_ptr() as u64,
        0u64,
    ];
    unsafe { syscalls::execve(shell_path.as_ptr(), argv.as_ptr(), envp.as_ptr()) }
}

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
        syscalls::write(
            1,
            b"[login] no users found - auto-login as root\n".as_ptr(),
            43,
        );
        seed::seed_root_account();
        let shell = b"/bin/osh\0";
        syscalls::write(
            1,
            b"[login] launching /bin/osh (root, ring 1)\n".as_ptr(),
            41,
        );
        exec_shell(b"root", shell);
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
        // Strip CR as well as LF: terminals (and the OC2R in-game terminal) send Enter
        // as '\r'; a trailing '\r' would corrupt the username match.
        let mut n = n;
        while n > 0 && (user_buf[n - 1] == b'\n' || user_buf[n - 1] == b'\r') {
            n -= 1;
        }
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

        // An empty submission (just Enter) is a valid password attempt: accounts
        // seeded with an empty password must be able to log in without typing one.
        if pn < 0 {
            backoff::backoff_sleep(fails);
            fails = fails.saturating_add(1);
            continue;
        }
        let pn = pn as usize;
        // Raw mode skips the kernel's CR/LF translation: Enter arrives as '\r', so it must
        // be stripped here or the password hash never matches.
        let mut pn = pn;
        while pn > 0 && (pass_buf[pn - 1] == b'\n' || pass_buf[pn - 1] == b'\r') {
            pn -= 1;
        }
        let password = &pass_buf[..pn];

        let outcome = auth::verify_shadow_outcome(username, password);
        if outcome == auth::VerifyOutcome::Fail {
            syscalls::write(1, b"Login incorrect\n\n".as_ptr(), 17);
            backoff::backoff_sleep(fails);
            fails = fails.saturating_add(1);
            continue;
        }

        // Transparent migration: if the entry matched only the legacy
        // single-round scheme, rewrite it in the current iterated $5$
        // format — but only from a root session, since rewriting
        // /etc/shadow needs write access (ACL allows uid==0).
        if outcome == auth::VerifyOutcome::OkLegacy && users[user_idx].uid == 0 {
            if auth::update_shadow_password(username, password).is_ok() {
                const MIG_MSG: &[u8] = b"[login] shadow entry migrated to iterated $5$ format\n";
                syscalls::write(1, MIG_MSG.as_ptr(), MIG_MSG.len());
            }
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
        let mut name_len = 0usize;
        while name_len < users[user_idx].name.len() && users[user_idx].name[name_len] != 0 {
            name_len += 1;
        }
        exec_shell(
            &users[user_idx].name[..name_len],
            &shell_path[..shell_len + 1],
        );
        syscalls::write(1, b"login: exec failed\n".as_ptr(), 19);
    }
}
