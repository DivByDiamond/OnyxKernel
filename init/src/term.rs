// TODO(2026-08-27): shared termios helper module, compiled per onyx_init bin
// via `#[path]` includes — same pattern as `auth`/`syscalls` — because
// onyx_init has no lib target yet.
//
// Secret line reading: the kernel's TIOCSRAW raw mode returns from read()
// as soon as at least ONE byte is available (POSIX VMIN=1/VTIME=0) and never
// waits for Enter. The historical callers (login/passwd/su) issued a single
// read() and treated whatever arrived as a complete line, so EVERY keypress
// submitted the whole "password" (bug report 2026-09-04, TumRedSun: passwd
// advanced a prompt per key, users got locked out because neither the new
// password nor the login password could be typed in full). The loop below
// reads byte-wise until Enter, mirrors each accepted char as '*' (raw mode
// has no kernel-side echo, so previously typing was completely invisible),
// supports backspace with on-screen erase, and restores cooked mode after.
#![allow(dead_code, unused_imports)]

use crate::syscalls;

/// ioctl numbers — match the kernel's `fs_sys3/extra.rs` definitions.
const TIOCSRAW: u64 = 0x5421;
const TIOCRRAW: u64 = 0x5422;
const FIONREAD: u64 = 0x541B;

const ERASE_SEQ: [u8; 3] = [0x08, b' ', 0x08];

/// Read a secret line from fd 0 (console) without local echo.
///
/// Switches the terminal to raw mode for the duration, consumes bytes until
/// Enter ('\r' or '\n' — raw mode skips the kernel's CR/LF translation),
/// mirrors every accepted character as '*', supports backspace (0x7F / 0x08)
/// with on-screen erase, and restores cooked mode before returning.
///
/// Printable ASCII (0x20..=0x7E) only; characters beyond `buf.len()` are
/// consumed but dropped so the buffer can never overflow. All other control
/// bytes are ignored. Returns the line without any terminator. The echo
/// always ends with '\n' so the next prompt starts on its own line.
///
/// # Bug fixed (2026-09-05)
///
/// A previous fix for the 2026-09-04 lockout bug (see above) added a
/// "drain leftover bytes from the previous call's cooked-mode echo" loop
/// that ran a BLOCKING read() at the start of every call after the first,
/// discarding everything read until it happened to see a `\n`/`\r`. When
/// there was no actual leftover byte (the common case — nothing loops a
/// guest's own TX back into its RX), this simply blocked until the user
/// started typing the CURRENT field and then silently discarded some or
/// all of those real keystrokes, corrupting multi-prompt flows (passwd's
/// "New password" / "Retype new password", and login's retry loop after a
/// failed attempt) — exactly the kind of data loss it was meant to fix.
/// Replaced with a narrow, non-blocking check via FIONREAD that only
/// swallows a genuine stray trailing `\n` completing a `\r\n` pair left
/// over from the previous field's Enter, and never blocks or discards
/// anything else.
pub unsafe fn read_secret_line(buf: &mut [u8]) -> &[u8] {
    let _ = syscalls::ioctl(0, TIOCSRAW, 0);

    // Non-blocking: if the previous field's Enter arrived as "\r\n" and
    // raw_read stopped at the '\r', the paired '\n' is still queued. Peek
    // via FIONREAD (never blocks) and consume it ONLY if it's actually
    // there and actually a bare '\n' — anything else is left untouched for
    // the real read loop below.
    let mut n = 0usize;
    let mut pending: u32 = 0;
    if syscalls::ioctl(0, FIONREAD, &mut pending as *mut u32 as u64) >= 0 && pending > 0 {
        let mut peek = [0u8; 1];
        if syscalls::read(0, peek.as_mut_ptr(), 1) == 1 && peek[0] != b'\n' {
            // Not the stray pair byte — it's real input; feed it back in.
            handle_byte(buf, &mut n, peek[0]);
        }
    }

    let mut chunk = [0u8; 32];
    loop {
        // Block for the first byte, then drain whatever else the kernel
        // already queued (pasted input arrives in bursts); repeat until the
        // user actually presses Enter.
        let r = syscalls::read(0, chunk.as_mut_ptr(), chunk.len() as u64);
        if r <= 0 {
            break;
        }
        let bytes = &chunk[..r as usize];
        let mut done = false;
        for &b in bytes {
            if b == b'\n' || b == b'\r' {
                done = true;
                break;
            }
            handle_byte(buf, &mut n, b);
        }
        if done {
            break;
        }
    }
    let _ = syscalls::ioctl(0, TIOCRRAW, 0);
    syscalls::write(1, b"\n".as_ptr(), b"\n".len());

    &buf[..n]
}

/// Applies one input byte to the in-progress secret line: backspace erases
/// the last accepted character (with on-screen erase), printable ASCII
/// appends and echoes '*', everything else is ignored.
unsafe fn handle_byte(buf: &mut [u8], n: &mut usize, b: u8) {
    if b == 0x7F || b == 0x08 {
        if *n > 0 {
            *n -= 1;
            syscalls::write(1, ERASE_SEQ.as_ptr(), ERASE_SEQ.len());
        }
    } else if (0x20..=0x7E).contains(&b) && *n < buf.len() {
        buf[*n] = b;
        *n += 1;
        syscalls::write(1, b"*".as_ptr(), b"*".len());
    }
    // other control bytes are ignored, never stored
}
