#![no_std]
#![no_main]
// TODO(2026-08-27): bin-root allow — raw syscall asm runs inside `unsafe fn`
// wrappers (no_std, per-bin compile); re-evaluate on toolchain/edition bump.
#![allow(unsafe_op_in_unsafe_fn)]

use core::arch::asm;

mod auth;
mod syscalls;
mod term;

#[unsafe(no_mangle)]
/// # Safety
///
/// Process entry point: called directly by the kernel from the ELF entry
/// address; the stack is freshly initialized per the RISC-V calling convention.
pub unsafe extern "C" fn _start() -> ! {
    let ring = syscalls::getring();
    if ring != 1 {
        syscalls::write(
            1,
            b"useradd: only root can add users\n".as_ptr(),
            b"useradd: only root can add users\n".len(),
        );
        syscalls::exit(1);
    }

    let mut username = [0u8; 32];
    syscalls::write(1, b"Username: ".as_ptr(), b"Username: ".len());
    let uname = read_line(&mut username);
    if uname.is_empty() {
        syscalls::write(
            1,
            b"useradd: no username\n".as_ptr(),
            b"useradd: no username\n".len(),
        );
        syscalls::exit(1);
    }
    // Audit fix (🔴 #6): validate the username before persisting it to
    // /etc/passwd. The previous code happily accepted any byte sequence,
    // including `:` (which splits fields) and `\n` (which adds a new
    // line — `evil\nhacker:0:0:/r:/bin/osh` would create a second uid-0
    // entry). We restrict to [A-Za-z0-9-_.], max 31 bytes, and refuse
    // the literal name "root" to prevent shadowing.
    if !valid_username(uname) {
        syscalls::write(
            1,
            b"useradd: invalid username\n".as_ptr(),
            b"useradd: invalid username\n".len(),
        );
        syscalls::exit(1);
    }

    let mut uid_str = [0u8; 12];
    syscalls::write(1, b"UID: ".as_ptr(), b"UID: ".len());
    let uid_s = read_line(&mut uid_str);
    let uid = parse_dec(uid_s);
    // Audit fix (🔴 #6): refuse uid 0 (would grant root), and refuse
    // a uid that's already taken by an existing user.
    if uid == 0 {
        syscalls::write(
            1,
            b"useradd: uid 0 is reserved for root\n".as_ptr(),
            b"useradd: uid 0 is reserved for root\n".len(),
        );
        syscalls::exit(1);
    }

    let mut password = [0u8; 64];
    syscalls::write(1, b"Password: ".as_ptr(), b"Password: ".len());
    // read_secret_line: cooked read_line() echoed the new user's password
    // onto the console in plaintext (same class of leak as audit fix 🟡 #2).
    // The helper also loops until Enter — one raw read() returns per
    // keypress, which broke every password prompt (2026-09-04 bug report).
    let pass = term::read_secret_line(&mut password);
    if pass.is_empty() {
        syscalls::write(
            1,
            b"useradd: no password\n".as_ptr(),
            b"useradd: no password\n".len(),
        );
        syscalls::exit(1);
    }

    // Check if user already exists
    let mut users = [auth::PasswdEntry {
        name: [0; 32],
        uid: 0,
        gid: 0,
        home: [0; 64],
        shell: [0; 32],
    }; auth::MAX_USERS];
    let nusers = auth::read_passwd(&mut users).unwrap_or(0);

    if auth::find_user(&users, nusers, uname).is_some() {
        syscalls::write(
            1,
            b"useradd: user already exists\n".as_ptr(),
            b"useradd: user already exists\n".len(),
        );
        syscalls::exit(1);
    }
    if auth::find_user_by_uid(&users, nusers, uid).is_some() {
        syscalls::write(
            1,
            b"useradd: uid already in use\n".as_ptr(),
            b"useradd: uid already in use\n".len(),
        );
        syscalls::exit(1);
    }

    // Build home path
    let mut home = [0u8; 64];
    home[..7].copy_from_slice(b"/users/");
    let n = uname.len().min(56);
    home[7..7 + n].copy_from_slice(&uname[..n]);

    let shell = b"/bin/osh";

    // Add entry
    let home_len = 7 + uname.len().min(56);
    if let Err(_e) = auth::update_passwd_entry(uname, uid, uid, &home[..home_len], shell) {
        syscalls::write(
            1,
            b"useradd: failed to update /etc/passwd\n".as_ptr(),
            b"useradd: failed to update /etc/passwd\n".len(),
        );
        syscalls::exit(1);
    }

    // Add shadow entry
    if let Err(_e) = auth::update_shadow_password(uname, pass) {
        syscalls::write(
            1,
            b"useradd: failed to update /etc/shadow\n".as_ptr(),
            b"useradd: failed to update /etc/shadow\n".len(),
        );
        syscalls::exit(1);
    }

    // Create home directory
    let mut mkdir_path = [0u8; 64];
    let np = home_len.min(63);
    mkdir_path[..np].copy_from_slice(&home[..np]);
    let ret = syscalls::mkdir(mkdir_path.as_ptr());
    if ret < 0 && ret != -13 {
        syscalls::write(
            1,
            b"useradd: warning: could not create home\n".as_ptr(),
            b"useradd: warning: could not create home\n".len(),
        );
    }

    syscalls::write(
        1,
        b"useradd: user added\n".as_ptr(),
        b"useradd: user added\n".len(),
    );
    syscalls::exit(0);
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

fn parse_dec(s: &[u8]) -> u32 {
    let mut val: u32 = 0;
    for &b in s.iter() {
        if b.is_ascii_digit() {
            val = val.wrapping_mul(10).wrapping_add(u32::from(b - b'0'));
        } else {
            break;
        }
    }
    val
}

/// Audit fix (🔴 #6): validate a username before writing it into /etc/passwd.
/// Rejects empty names, names longer than 31 bytes, names containing
/// anything outside [A-Za-z0-9-_.] (so `:` and `\n` are forbidden), and
/// the literal name "root".
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
