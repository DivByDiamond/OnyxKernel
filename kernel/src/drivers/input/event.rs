//! Unified input subsystem — device-independent event types + dispatch.
//!
//! Provides a single `Event` type that abstracts over keyboards, mice,
//! tablets, and any future input source. Drivers (virtio-input, PS/2,
//! GPIO buttons) call `dispatch()` to publish events; consumers (init,
//! shell, GUI) register a handler via `on_event()`.
//!
//! The handler table is intentionally tiny: one global callback. If
//! multiple subscribers are needed in the future, extend with a small
//! array of `Option<Handler>` slots.
use super::{mouse, tty};
use crate::drivers::virtio_input;

/// Logical key code. Extend as more keys are needed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeyCode {
    Unknown,
    Esc,
    Enter,
    Backspace,
    Tab,
    Space,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
    LeftShift,
    RightShift,
    LeftCtrl,
    RightCtrl,
    LeftAlt,
    RightAlt,
    CapsLock,
    NumLock,
    ScrollLock,
    Letter(u8),
    Digit(u8),
    F(u8),
    Minus,
    Equal,
    LeftBrace,
    RightBrace,
    Semicolon,
    Apostrophe,
    Grave,
    Backslash,
    Comma,
    Dot,
    Slash,
}

/// Mouse buttons.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Other(u8),
}

/// Unified input event.
#[derive(Clone, Copy, Debug)]
pub enum Event {
    Key { code: KeyCode, down: bool },
    MouseRel { dx: i32, dy: i32 },
    MouseAbs { x: i32, y: i32 },
    MouseButton { btn: MouseButton, down: bool },
    Syn,
}

pub type Handler = fn(Event);

static mut G_HANDLER: Option<Handler> = None;
use core::sync::atomic::{AtomicU8, Ordering};
// Separate left/right modifier bits keep either held key active across devices.
static MODIFIERS: AtomicU8 = AtomicU8::new(0);
fn update_modifiers(flags: &AtomicU8, code: KeyCode, down: bool) {
    let bit = match code {
        KeyCode::LeftShift => 1,
        KeyCode::RightShift => 2,
        KeyCode::LeftCtrl => 4,
        KeyCode::RightCtrl => 8,
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

/// Register a single global input handler. Replaces any previous handler.
pub fn on_event(h: Handler) {
    // SAFETY: G_HANDLER is a plain kernel-owned slot; registration happens single-threaded at init (SIE=0).
    unsafe {
        G_HANDLER = Some(h);
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

/// Dispatch one event: mouse model, TTY FIFO (for Key down), then handler.
pub fn dispatch(ev: Event) {
    if let Event::Key { code, down } = ev {
        update_modifiers(&MODIFIERS, code, down);
        if down {
            push_key(code);
        }
    }
    mouse::handle(ev);
    // SAFETY: G_HANDLER is read and the fn(Event) pointer is invoked only if registered; registration raced nothing (kernel never runs with SIE set).
    unsafe {
        if let Some(h) = G_HANDLER {
            h(ev);
        }
    }
}

/// Poll every known input source and dispatch any events found.
/// Drains the virtio-input queue completely so a burst of keystrokes is
/// not lost between ticks (one tick = 10ms at 100 Hz).
static DRAIN_LOCK: crate::sync::SpinLock = crate::sync::SpinLock::new();

fn drain_with(
    lock: &crate::sync::SpinLock,
    mut poll: impl FnMut() -> Option<virtio_input::EventType>,
    mut output: impl FnMut(Event),
) {
    // One consumer must dequeue AND dispatch in order. A competing/reentrant
    // syscall may observe the already published state and retry on its next poll.
    if !lock.try_lock() {
        return;
    }
    while let Some(ev) = poll() {
        if let Some(ev) = virtio_input::event_type(ev) {
            output(ev);
        }
    }
    lock.unlock();
}

pub fn poll_all() {
    drain_with(&DRAIN_LOCK, virtio_input::poll, dispatch);
}

/// Was a handler registered?
pub fn has_handler() -> bool {
    // SAFETY: G_HANDLER is a plain kernel-owned slot; read without concurrency (kernel never runs with SIE set).
    unsafe { G_HANDLER.is_some() }
}

#[cfg(test)]
mod modifier_tests {
    use super::*;
    #[test]
    fn a_reentrant_drain_cannot_dispatch_newer_events_before_the_current_one() {
        let lock = crate::sync::SpinLock::new();
        let mut events = [
            virtio_input::EventType::Key(KeyCode::LeftShift, true),
            virtio_input::EventType::Key(KeyCode::Letter(b'a'), true),
        ]
        .into_iter();
        let mut received = std::vec::Vec::new();
        drain_with(
            &lock,
            || events.next(),
            |ev| {
                drain_with(
                    &lock,
                    || panic!("reentrant poll must not consume events"),
                    |_| panic!(),
                );
                if let Event::Key { code, .. } = ev {
                    received.push(code);
                }
            },
        );
        assert_eq!(received, [KeyCode::LeftShift, KeyCode::Letter(b'a')]);
        assert!(lock.try_lock());
        lock.unlock();
    }

    #[test]
    fn either_shift_and_ctrl_remain_active_until_both_sides_are_released() {
        let flags = AtomicU8::new(0);
        update_modifiers(&flags, KeyCode::LeftShift, true);
        update_modifiers(&flags, KeyCode::RightShift, true);
        update_modifiers(&flags, KeyCode::LeftShift, false);
        assert_ne!(flags.load(Ordering::Acquire) & 3, 0);
        update_modifiers(&flags, KeyCode::RightShift, false);
        assert_eq!(flags.load(Ordering::Acquire) & 3, 0);
        update_modifiers(&flags, KeyCode::LeftCtrl, true);
        update_modifiers(&flags, KeyCode::RightCtrl, true);
        update_modifiers(&flags, KeyCode::RightCtrl, false);
        assert_ne!(flags.load(Ordering::Acquire) & 12, 0);
        update_modifiers(&flags, KeyCode::LeftCtrl, false);
        assert_eq!(flags.load(Ordering::Acquire), 0);
    }
}
