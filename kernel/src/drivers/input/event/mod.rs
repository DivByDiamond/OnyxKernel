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
//!
//! Module layout (responsibility split):
//! - this file: public event types, handler registration and dispatch;
//! - [`key`]: modifier tracking and key → TTY byte translation.
use super::mouse;
use crate::drivers::virtio_input;

mod key;
#[cfg(test)]
mod tests;

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
    MouseAbsX(i32),
    MouseAbsY(i32),
    MouseButton { btn: MouseButton, down: bool },
    Syn,
}

pub type Handler = fn(Event);

static mut G_HANDLER: Option<Handler> = None;

/// Register a single global input handler. Replaces any previous handler.
pub fn on_event(h: Handler) {
    // SAFETY: G_HANDLER is a plain kernel-owned slot; registration happens single-threaded at init (SIE=0).
    unsafe {
        G_HANDLER = Some(h);
    }
}

/// Dispatch one event: mouse model, TTY FIFO (for Key down), then handler.
pub fn dispatch(ev: Event) {
    if let Event::Key { code, down } = ev {
        key::handle_key(code, down);
    }
    mouse::handle(ev);
    // SAFETY: G_HANDLER is read and the fn(Event) pointer is invoked only if registered; registration raced nothing (kernel never runs with SIE set).
    unsafe {
        if let Some(h) = G_HANDLER {
            h(ev);
        }
    }
}

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

/// Poll every known input source and dispatch any events found.
/// Drains the virtio-input queue completely so a burst of keystrokes is
/// not lost between ticks (one tick = 10ms at 100 Hz).
pub fn poll_all() {
    drain_with(&DRAIN_LOCK, virtio_input::poll, dispatch);
}

/// Was a handler registered?
pub fn has_handler() -> bool {
    // SAFETY: G_HANDLER is a plain kernel-owned slot; read without concurrency (kernel never runs with SIE set).
    unsafe { G_HANDLER.is_some() }
}
