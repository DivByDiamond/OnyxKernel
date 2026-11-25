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
//! - [`key`]: modifier tracking and key → TTY byte translation;
//! - `hotkey` (sibling module): chords intercepted before [`key`].
use super::mouse;
use crate::drivers::bus::usb::hid::{self, HidEvent};
use crate::drivers::virtio_input;

mod key;
pub mod stick_cursor;
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
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Event {
    Key {
        code: KeyCode,
        down: bool,
    },
    MouseRel {
        dx: i32,
        dy: i32,
    },
    MouseAbs {
        x: i32,
        y: i32,
    },
    MouseAbsX(i32),
    MouseAbsY(i32),
    MouseButton {
        btn: MouseButton,
        down: bool,
    },
    /// HID stick/joystick axis (axis id = first-appearance order in
    /// the report descriptor). Consumed by the global handler; while
    /// the cursor is unclaimed, [`stick_cursor::map`] also translates
    /// axes 0/1 into [`Event::MouseRel`].
    Stick {
        axis: u8,
        value: i16,
    },
    /// HID stick button edge (button index minus one).
    StickButton {
        index: u8,
        down: bool,
    },
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

/// Dispatch one event: hotkey intercept, mouse model, TTY FIFO (for Key
/// down), then handler. A key that completes a registered hotkey chord is
/// swallowed before `key::handle_key`: it never reaches the TTY, but the
/// handler still sees the raw event.
pub fn dispatch(ev: Event) {
    if let Event::Key { code, down } = ev
        && !crate::drivers::input::hotkey::intercept(code, down, key::flags())
    {
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

/// Feed one HID event into the unified stream: first the /dev/jsN ring
/// (the raw event form — it fills regardless of claim state), then the
/// raw `Stick` event to the global handler, then, while the cursor is
/// unclaimed (per slot), the mapped mouse event.
///
/// `slot` is the attached session's registry slot, carried on the poll
/// result: the ring push routes by the session that produced the
/// event, never by registry class — a HID-class slot without a
/// session (the dual-controller keyboard case) must not steal the
/// stick's records.
fn feed_stick(hev: HidEvent, is_init: bool, slot: usize, output: impl FnMut(Event)) {
    // Ring push before any claim gating (ruling: every polled event
    // lands in the attached session's ring). Out-of-range slots are
    // ignored inside push_event — the drain only ever carries slot
    // ids the session layer validated at attach time.
    crate::fs::devfs::push_event(slot, hev, is_init);
    forward_stick(hev, slot, output);
}

/// The non-ring half of [`feed_stick`]: the raw `Stick*` event, then
/// the cursor-mapped event while slot `slot`'s stick is unclaimed.
/// Split out so claim gating is testable without touching a ring.
fn forward_stick(hev: HidEvent, slot: usize, mut output: impl FnMut(Event)) {
    let raw = match hev {
        HidEvent::Axis { axis, value } => Event::Stick { axis, value },
        HidEvent::Button { index, down } => Event::StickButton { index, down },
    };
    output(raw);
    if !stick_cursor::claimed(slot)
        && let Some(mapped) = stick_cursor::map(hev)
    {
        output(mapped);
    }
}

static DRAIN_LOCK: crate::sync::SpinLock = crate::sync::SpinLock::new();

/// Serializes the tests that mutate the stick-claim counters (the
/// stick_cursor integration test and the devfs js hooks test), so
/// parallel test harts cannot interleave claim/release calls — the
/// net::lock::test_serial pattern.
#[cfg(test)]
pub(crate) static CLAIM_TEST_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Drain `poll` completely, feeding each item to `output` in order.
fn drain_with<T>(mut poll: impl FnMut() -> Option<T>, mut output: impl FnMut(T)) {
    while let Some(ev) = poll() {
        output(ev);
    }
}

/// Run one exclusive input-drain pass. A competing/reentrant consumer
/// returns immediately: it may observe the already published state
/// and retry on its next poll — interleaved dispatch would reorder events.
fn with_drain_lock(lock: &crate::sync::SpinLock, body: impl FnOnce()) {
    if !lock.try_lock() {
        return;
    }
    body();
    lock.unlock();
}

/// virtio-input source: raw queue events translated to the unified form.
fn drain_virtio() {
    drain_with(virtio_input::poll, |raw| {
        if let Some(ev) = virtio_input::event_type(raw) {
            dispatch(ev);
        }
    });
}

/// USB HID source: every polled event goes through [`feed_stick`]. The
/// is-init flag and the producing session's slot ride into the
/// /dev/jsN ring; dispatch ignores both.
fn drain_hid() {
    drain_with(hid::poll, |(hev, is_init, slot)| {
        feed_stick(hev, is_init, slot, dispatch)
    });
}

/// Poll every known input source and dispatch any events found.
/// Drains both queues completely so a burst of events is not lost
/// between ticks (one tick = 10ms at 100 Hz), all under one
/// DRAIN_LOCK pass.
pub fn poll_all() {
    with_drain_lock(&DRAIN_LOCK, || {
        drain_virtio();
        drain_hid();
    });
}

/// Was a handler registered?
pub fn has_handler() -> bool {
    // SAFETY: G_HANDLER is a plain kernel-owned slot; read without concurrency (kernel never runs with SIE set).
    unsafe { G_HANDLER.is_some() }
}
