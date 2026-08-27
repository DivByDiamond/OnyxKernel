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

/// Register a single global input handler. Replaces any previous handler.
pub fn on_event(h: Handler) {
    unsafe {
        G_HANDLER = Some(h);
    }
}

/// Dispatch one event to the registered handler (if any).
pub fn dispatch(ev: Event) {
    unsafe {
        if let Some(h) = G_HANDLER {
            h(ev);
        }
    }
}

/// Poll every known input source once and dispatch any events found.
/// Intended to be called from the timer tick or an idle loop.
pub fn poll_all() {
    // virtio-input first (if present).
    if let Some(ev) = virtio_input::poll_unified() {
        dispatch(ev);
    }
    // PS/2 polled in drivers::ps2 — call from there directly when present.
}

/// Was a handler registered?
pub fn has_handler() -> bool {
    unsafe { G_HANDLER.is_some() }
}
