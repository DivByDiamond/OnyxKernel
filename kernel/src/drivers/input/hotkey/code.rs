//! Wire encoding of hotkey chords (ioctl argument) and the key-code map.
//!
//! `FB_IOCTL_HOTKEY_SET` packs a chord into one u64, so no user pointer is
//! involved: `id` (bits 0..8), `mods` (bits 8..16, `MOD_*`), `code`
//! (bits 16..32, see [`decode_code`]).

use super::super::event::KeyCode;

/// Modifier bits of a chord (the wire form, not the key.rs flag layout).
pub const MOD_SHIFT: u8 = 1;
pub const MOD_CTRL: u8 = 2;
pub const MOD_ALT: u8 = 4;
const MOD_ALL: u8 = MOD_SHIFT | MOD_CTRL | MOD_ALT;

/// Key codes: ASCII for letters (`a`..`z`), digits and Tab/Esc/Enter/
/// Space/Backspace; `0x100 + n` for F1..F12. Modifier keys are not
/// bindable (they only qualify other keys).
pub const CODE_F_BASE: u16 = 0x100;

/// A decoded `FB_IOCTL_HOTKEY_SET` argument.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Chord {
    pub id: u8,
    pub mods: u8,
    pub code: KeyCode,
}

/// Map a wire key code to a [`KeyCode`]; unknown codes are `None`.
pub fn decode_code(code: u16) -> Option<KeyCode> {
    match code {
        0x08 => Some(KeyCode::Backspace),
        0x09 => Some(KeyCode::Tab),
        0x0A => Some(KeyCode::Enter),
        0x1B => Some(KeyCode::Esc),
        0x20 => Some(KeyCode::Space),
        0x30..=0x39 => Some(KeyCode::Digit(code as u8)),
        0x61..=0x7A => Some(KeyCode::Letter(code as u8)),
        c if (CODE_F_BASE + 1..=CODE_F_BASE + 12).contains(&c) => {
            Some(KeyCode::F((c - CODE_F_BASE) as u8))
        }
        _ => None,
    }
}

/// Decode a packed chord. `id == 0` (reserved for "no event"), unknown
/// modifier bits and unknown key codes are rejected.
pub fn decode_chord(arg: u64) -> Option<Chord> {
    let id = (arg & 0xFF) as u8;
    let mods = ((arg >> 8) & 0xFF) as u8;
    let code = decode_code(((arg >> 16) & 0xFFFF) as u16)?;
    if id == 0 || mods & !MOD_ALL != 0 || arg >> 32 != 0 {
        return None;
    }
    Some(Chord { id, mods, code })
}
