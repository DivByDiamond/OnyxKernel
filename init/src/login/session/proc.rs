//! Process-lifecycle helpers for `/bin/login`: draining stray stdin bytes
//! before the first prompt, and building argv/envp to exec the user's shell.

use crate::syscalls;

// Local copies of the kernel ABI constants this file needs (init binaries
// are separate no_std crates and don't link kernel::syscall::abi).
const F_GETFL: u32 = 3;
const F_SETFL: u32 = 4;
const O_NONBLOCK: u64 = 1 << 11;

/// Discard any bytes already queued on stdin before the very first "login: "
/// prompt of this process's life.
///
/// Root cause (2026-09-12 bug report): the first login attempt right after
/// boot intermittently failed with "Login incorrect" WITHOUT ever reaching
/// the password prompt, while an identical second attempt worked. That
/// means `find_user` rejected a non-empty username that LOOKED like "root"
/// to the person typing it — i.e. the actual bytes read() returned differed
/// from what they typed. A byte (or an escape sequence — e.g. a terminal's
/// automatic focus-report on window-focus change, or key autorepeat) queued
/// in the UART FIFO before this process's first ever read() call gets
/// silently prepended to the real "root" the user then types, since
/// `cooked_read` has no way to distinguish stale pre-existing input from
/// fresh keystrokes. The existing bare-`\n`/`\r` case was already handled
/// (see the `username.is_empty()` comment below), but that only covers a
/// stray byte that happens to BE a line terminator — any other leftover
/// byte silently corrupts the username instead. Draining once, right
/// before the first prompt, discards whatever garbage accumulated during
/// boot regardless of its content, without touching later retries within
/// this same session (so a user typing ahead during the backoff sleep
/// after a genuinely wrong password is never eaten).
pub unsafe fn drain_stdin() {
    unsafe {
        let flags = syscalls::fcntl(0, F_GETFL, 0);
        if flags < 0 {
            return;
        }
        let _ = syscalls::fcntl(0, F_SETFL, (flags as u64) | O_NONBLOCK);
        let mut junk = [0u8; 64];
        loop {
            let n = syscalls::read(0, junk.as_mut_ptr(), junk.len() as u64);
            if n <= 0 {
                break;
            }
        }
        let _ = syscalls::fcntl(0, F_SETFL, flags as u64);
    }
}

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
pub fn exec_shell(username: &[u8], shell_path: &[u8]) -> i64 {
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
