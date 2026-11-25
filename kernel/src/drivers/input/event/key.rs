//! Keyboard handling: modifier tracking and key → TTY byte translation.
//!
//! Modifier state is a single atomic bitmask shared by all keyboard devices,
//! so releasing Shift on one device does not clear it while the other side
//! is still held.
use super::KeyCode;
use crate::drivers::input::tty;
use core::sync::atomic::{AtomicU8, Ordering};

// Separate left/right modifier bits keep either held key active across devices.
static MODIFIERS: AtomicU8 = AtomicU8::new(0);

pub(super) fn update_modifiers(flags: &AtomicU8, code: KeyCode, down: bool) {
    let bit = match code {
        KeyCode::LeftShift => 1,
        KeyCode::RightShift => 2,
        KeyCode::LeftCtrl => 4,
        KeyCode::RightCtrl => 8,
        KeyCode::LeftAlt => 32,
        KeyCode::RightAlt => 64,
        KeyCode::CapsLock if down => {
            flags.fetch_xor(16, Ordering::AcqRel);
            return;
        }
        _ => return,
    };
    if down {
        flags.fetch_or(bit, Ordering::AcqRel);
    } else {
        flags.fetch_and(!bit, Ordering::AcqRel);
    }
}

/// Current modifier flag byte (shift 1|2, ctrl 4|8, caps 16, alt 32|64);
/// the hotkey intercept maps it to chord bits.
pub(super) fn flags() -> u8 {
    MODIFIERS.load(Ordering::Acquire)
}

/// Track a key event and, on press, feed its TTY bytes.
pub(super) fn handle_key(code: KeyCode, down: bool) {
    update_modifiers(&MODIFIERS, code, down);
    if down {
        push_key(code);
    }
}

fn key_to_byte(code: KeyCode) -> Option<u8> {
    let flags = MODIFIERS.load(Ordering::Acquire);
    let shift = flags & 3 != 0;
    let ctrl = flags & 12 != 0;
    let caps = flags & 16 != 0;
    match code {
        KeyCode::Letter(b) => {
            let upper = shift ^ caps;
            let out = if upper { b.to_ascii_uppercase() } else { b };
            if ctrl {
                let low = out.to_ascii_lowercase();
                if low.is_ascii_lowercase() {
                    return Some(low - b'a' + 1);
                }
                return None;
            }
            Some(out)
        }
        KeyCode::Digit(b) => {
            if shift {
                let sym = match b {
                    b'1' => b'!',
                    b'2' => b'@',
                    b'3' => b'#',
                    b'4' => b'$',
                    b'5' => b'%',
                    b'6' => b'^',
                    b'7' => b'&',
                    b'8' => b'*',
                    b'9' => b'(',
                    b'0' => b')',
                    _ => b,
                };
                Some(sym)
            } else {
                Some(b)
            }
        }
        KeyCode::Enter => Some(b'\n'),
        KeyCode::Space => Some(b' '),
        KeyCode::Tab => Some(b'\t'),
        KeyCode::Backspace => Some(0x7F),
        KeyCode::Esc => Some(0x1B),
        KeyCode::Minus => Some(if shift { b'_' } else { b'-' }),
        KeyCode::Equal => Some(if shift { b'+' } else { b'=' }),
        KeyCode::LeftBrace => Some(if shift { b'{' } else { b'[' }),
        KeyCode::RightBrace => Some(if shift { b'}' } else { b']' }),
        KeyCode::Semicolon => Some(if shift { b':' } else { b';' }),
        KeyCode::Apostrophe => Some(if shift { b'"' } else { b'\'' }),
        KeyCode::Grave => Some(if shift { b'~' } else { b'`' }),
        KeyCode::Backslash => Some(if shift { b'|' } else { b'\\' }),
        KeyCode::Comma => Some(if shift { b'<' } else { b',' }),
        KeyCode::Dot => Some(if shift { b'>' } else { b'.' }),
        KeyCode::Slash => Some(if shift { b'?' } else { b'/' }),
        _ => None,
    }
}

fn special_key_seq(code: KeyCode) -> Option<&'static [u8]> {
    match code {
        KeyCode::Up => Some(b"\x1b[A"),
        KeyCode::Down => Some(b"\x1b[B"),
        KeyCode::Right => Some(b"\x1b[C"),
        KeyCode::Left => Some(b"\x1b[D"),
        KeyCode::Home => Some(b"\x1b[1~"),
        KeyCode::Insert => Some(b"\x1b[2~"),
        KeyCode::Delete => Some(b"\x1b[3~"),
        KeyCode::End => Some(b"\x1b[4~"),
        KeyCode::PageUp => Some(b"\x1b[5~"),
        KeyCode::PageDown => Some(b"\x1b[6~"),
        KeyCode::F(1) => Some(b"\x1bOP"),
        KeyCode::F(2) => Some(b"\x1bOQ"),
        KeyCode::F(3) => Some(b"\x1bOR"),
        KeyCode::F(4) => Some(b"\x1bOS"),
        KeyCode::F(5) => Some(b"\x1b[15~"),
        KeyCode::F(6) => Some(b"\x1b[17~"),
        KeyCode::F(7) => Some(b"\x1b[18~"),
        KeyCode::F(8) => Some(b"\x1b[19~"),
        KeyCode::F(9) => Some(b"\x1b[20~"),
        KeyCode::F(10) => Some(b"\x1b[21~"),
        KeyCode::F(11) => Some(b"\x1b[23~"),
        KeyCode::F(12) => Some(b"\x1b[24~"),
        _ => None,
    }
}

fn push_key(code: KeyCode) {
    if let Some(b) = key_to_byte(code) {
        let _ = tty::push(b);
        return;
    }
    if let Some(seq) = special_key_seq(code) {
        let _ = tty::push_slice(seq);
    }
}
