//! PS/2 controller (i8042) — keyboard via the legacy 8042.
//!
//! Useful when QEMU is started with `-keyboard` or on KVM boards where
//! the 8042 is still present. The driver polls the data port (0x60) and
//! translates scancode set 2 (translated by the 8042 to set 1) to the
//! unified `input::Event` representation.
use crate::arch::mmio::Mmio;
use crate::drivers::input::{self, Event, KeyCode};

const PORT_DATA: usize = 0x60;
const PORT_STATUS: usize = 0x64;
const PORT_COMMAND: usize = 0x64;

const SR_OUT_BUF: u32 = 1 << 0;
const _SR_IN_BUF: u32 = 1 << 1;
const _SR_SYS_FLAG: u32 = 1 << 2;
const _SR_CMD_DATA: u32 = 1 << 3;
const _SR_LOCK: u32 = 1 << 5;
const _SR_TIMEOUT: u32 = 1 << 6;
const _SR_PARITY: u32 = 1 << 7;

static mut G_SHIFT: bool = false;
static mut G_CAPS: bool = false;

#[inline]
/// # Safety
///
/// Caller contract: reads the fixed i8042 status port; must only be
/// called in kernel context where the 8042 is present.
unsafe fn status() -> u32 {
    // SAFETY: PORT_STATUS (0x64) is the fixed legacy i8042 port constant defined in this file; a u32 read of that port is within the controller's register window.
    unsafe { Mmio::<u32>::at(PORT_STATUS).read() }
}

#[inline]
/// # Safety
///
/// Caller contract: must be called only after `status()` showed the
/// output-buffer-full bit set, so the data port actually holds a byte.
unsafe fn data() -> u8 {
    // SAFETY: PORT_DATA (0x60) is the fixed legacy i8042 port constant defined in this file; a read of that port is within the controller's register window.
    unsafe { Mmio::<u32>::at(PORT_DATA).read() as u8 }
}

#[inline]
/// # Safety
///
/// Caller contract: kernel context with the 8042 present; spins on MMIO
/// reads only.
unsafe fn wait_read() -> bool {
    // SAFETY: status() reads the fixed i8042 status port constant; the bounded spin touches no other unsafe state.
    unsafe {
        let mut t = 100_000u32;
        while t > 0 {
            if status() & SR_OUT_BUF != 0 {
                return true;
            }
            t -= 1;
        }
        false
    }
}

/// Initialise the controller. Disables both ports, flushes the output
/// buffer, then re-enables the keyboard port. Returns false on timeout.
pub unsafe fn init() -> bool {
    // SAFETY: command/data writes go to the fixed i8042 port constants (0x60/0x64) defined in this file; runs single-threaded at driver init (SIE=0).
    unsafe {
        // Disable both ports.
        Mmio::<u32>::at(PORT_COMMAND).write(0xAD);
        Mmio::<u32>::at(PORT_COMMAND).write(0xA7);
        // Flush output buffer.
        while status() & SR_OUT_BUF != 0 {
            let _ = data();
        }
        // Self-test.
        Mmio::<u32>::at(PORT_COMMAND).write(0xAA);
        if !wait_read() || data() != 0x55 {
            return false;
        }
        // Re-enable keyboard port.
        Mmio::<u32>::at(PORT_COMMAND).write(0xAE);
        true
    }
}

/// Poll the PS/2 keyboard for one byte. Returns `None` if no data.
pub fn poll_byte() -> Option<u8> {
    // SAFETY: wait_read()/data() access only the fixed i8042 port constants defined in this file.
    unsafe {
        if !wait_read() {
            return None;
        }
        Some(data())
    }
}

/// Poll and translate one scancode to a unified `Event`.
pub fn poll() -> Option<Event> {
    Some(translate(poll_byte()?))
}

/// Convert a translated set-1 scancode into an `Event`.
fn translate(sc: u8) -> Event {
    let down = sc & 0x80 == 0; // bit 7 set = break
    let code = sc & 0x7F;
    let kc = scancode_to_key(code);
    if matches!(kc, KeyCode::LeftShift | KeyCode::RightShift) {
        // SAFETY: G_SHIFT is a plain kernel-owned flag; scancode translation is single-threaded poll context (SIE=0).
        unsafe {
            G_SHIFT = down;
        }
    }
    if kc == KeyCode::CapsLock && down {
        // SAFETY: G_CAPS is a plain kernel-owned flag; scancode translation is single-threaded poll context (SIE=0).
        unsafe {
            G_CAPS = !G_CAPS;
        }
    }
    Event::Key { code: kc, down }
}

/// Translate a set-1 (host-translated) scancode to a `KeyCode`.
fn scancode_to_key(code: u8) -> KeyCode {
    match code {
        1 => KeyCode::Esc,
        14 => KeyCode::Backspace,
        15 => KeyCode::Tab,
        28 => KeyCode::Enter,
        29 => KeyCode::LeftCtrl,
        42 => KeyCode::LeftShift,
        54 => KeyCode::RightShift,
        56 => KeyCode::LeftAlt,
        57 => KeyCode::Space,
        72 => KeyCode::Up,
        75 => KeyCode::Left,
        77 => KeyCode::Right,
        80 => KeyCode::Down,
        58 => KeyCode::CapsLock,
        59..=68 => KeyCode::F(code - 58),
        // Letters / digits — apply case if shift or caps lock.
        c if (2..=11).contains(&c) => {
            let n = (c - 1) % 10;
            let digit = if unsafe {
                // SAFETY: G_SHIFT is a plain kernel-owned flag; scancode translation is single-threaded poll context (SIE=0).
                G_SHIFT
            } {
                b")!@#$%^&*("[n as usize]
            } else {
                b'0' + n
            };
            KeyCode::Digit(digit)
        }
        c if (16..=25).contains(&c) => {
            let base = b'q' + (c - 16);
            KeyCode::Letter(apply_case(base))
        }
        c if (30..=38).contains(&c) => {
            let base = b'a' + (c - 30);
            KeyCode::Letter(apply_case(base))
        }
        _ => KeyCode::Unknown,
    }
}

#[inline]
fn apply_case(base: u8) -> u8 {
    // SAFETY: G_SHIFT/G_CAPS are plain kernel-owned flags; scancode translation is single-threaded poll context (SIE=0).
    let up = unsafe { G_SHIFT ^ G_CAPS };
    if up { base.to_ascii_uppercase() } else { base }
}

/// Drive `input::dispatch` from the PS/2 source. Call from a poll loop.
pub fn dispatch_into_input() {
    if let Some(ev) = poll() {
        input::dispatch(ev);
    }
}
