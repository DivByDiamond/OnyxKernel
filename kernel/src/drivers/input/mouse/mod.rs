//! Mouse cursor state (todo P3 #1): accumulates unified input events into
//! the bounded button history and current state SYS_mouse_read exposes.
//!
//! The kernel tracks exactly one logical cursor: relative moves integrate
//! into absolute coordinates clamped to the visible framebuffer (or the
//! 640x480 default when no display is up), absolute moves replace them,
//! button bits follow press/release pairs. All state is kernel-owned and
//! only mutated from the input dispatch path (kernel context, SIE clear).

use super::event::Event;

const DEFAULT_W: i32 = 640;
const DEFAULT_H: i32 = 480;

/// Button bits in MouseEvent (bit 0 left, 1 right, 2 middle).
const BTN_LEFT: u8 = 1;
const BTN_RIGHT: u8 = 2;
const BTN_MIDDLE: u8 = 4;
static MOUSE_LOCK: crate::sync::SpinLock = crate::sync::SpinLock::new();
fn button_bit(button: super::event::MouseButton) -> u8 {
    match button {
        super::event::MouseButton::Left => BTN_LEFT,
        super::event::MouseButton::Right => BTN_RIGHT,
        super::event::MouseButton::Middle => BTN_MIDDLE,
        super::event::MouseButton::Other(b) if (3..8).contains(&b) => 1 << b,
        _ => 0,
    }
}

#[derive(Clone, Copy)]
struct MouseState {
    x: i32,
    y: i32,
    buttons: u8,
}

const EMPTY: MouseState = MouseState {
    x: 0,
    y: 0,
    buttons: 0,
};
const EDGES: usize = 32;
struct PointerQueue {
    latest: MouseState,
    edges: [MouseState; EDGES],
    head: usize,
    len: usize,
    overflow: bool,
}
impl PointerQueue {
    const fn new() -> Self {
        Self {
            latest: EMPTY,
            edges: [EMPTY; EDGES],
            head: 0,
            len: 0,
            overflow: false,
        }
    }
    fn handle(&mut self, ev: Event, w: i32, h: i32) {
        let previous = self.latest.buttons;
        apply(&mut self.latest, ev, w, h);
        if previous != self.latest.buttons {
            if self.len == EDGES {
                // Cancel an incomplete gesture rather than inventing a click
                // after bounded input history is exhausted.
                self.head = 0;
                self.len = 0;
                self.overflow = true;
            }
            self.edges[(self.head + self.len) % EDGES] = self.latest;
            self.len += 1;
        }
    }
    fn read(&mut self) -> MouseState {
        if self.overflow {
            self.overflow = false;
            return MouseState {
                x: -1,
                y: -1,
                buttons: 0,
            };
        }
        if self.len == 0 {
            return self.latest;
        }
        let edge = self.edges[self.head];
        self.head = (self.head + 1) % EDGES;
        self.len -= 1;
        edge
    }
}
static mut G_MOUSE: PointerQueue = PointerQueue::new();

/// Visible extents for clamping (fb geometry when available).
fn bounds() -> (i32, i32) {
    if crate::drivers::fb::enabled() {
        (
            crate::drivers::fb::width() as i32,
            crate::drivers::fb::height() as i32,
        )
    } else {
        (DEFAULT_W, DEFAULT_H)
    }
}

fn clamp(v: i32, max: i32) -> i32 {
    v.clamp(0, max.saturating_sub(1))
}

/// Feed one unified event into the cursor model. Events that are not
/// mouse-related are ignored; called from input::dispatch.
pub fn handle(ev: Event) {
    let (w, h) = bounds();
    MOUSE_LOCK.lock();
    // SAFETY: lock serializes cross-hart dispatch and snapshot reads;
    // callers run with interrupts disabled, as required by sync.rs.
    unsafe {
        let m = &raw mut G_MOUSE;
        (*m).handle(ev, w, h);
    }
    MOUSE_LOCK.unlock();
}

fn apply(m: &mut MouseState, ev: Event, w: i32, h: i32) {
    match ev {
        Event::MouseRel { dx, dy } => {
            m.x = clamp(m.x + dx, w);
            m.y = clamp(m.y + dy, h);
        }
        Event::MouseAbs { x, y } => {
            m.x = clamp(x, w);
            m.y = clamp(y, h);
        }
        Event::MouseAbsX(x) => m.x = clamp(x, w),
        Event::MouseAbsY(y) => m.y = clamp(y, h),
        Event::MouseButton { btn, down } => {
            let bit = button_bit(btn);
            if down {
                m.buttons |= bit;
            } else {
                m.buttons &= !bit;
            }
        }
        _ => {}
    }
}

/// Oldest unread button edge, otherwise current pointer state. Motion is
/// coalesced; captured press/release coordinates survive slow guest redraws.
pub fn snapshot() -> (i16, i16, u8) {
    MOUSE_LOCK.lock();
    // SAFETY: lock excludes all mutations by input dispatch.
    let value = unsafe {
        let m = (&mut *(&raw mut G_MOUSE)).read();
        (m.x as i16, m.y as i16, m.buttons)
    };
    MOUSE_LOCK.unlock();
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn short_click_survives_motion_and_two_edges_between_reads() {
        let mut q = PointerQueue::new();
        q.handle(Event::MouseAbs { x: 30, y: 98 }, 256, 160);
        q.handle(
            Event::MouseButton {
                btn: super::super::event::MouseButton::Left,
                down: true,
            },
            256,
            160,
        );
        q.handle(Event::MouseAbsX(31), 256, 160);
        q.handle(
            Event::MouseButton {
                btn: super::super::event::MouseButton::Left,
                down: false,
            },
            256,
            160,
        );
        let press = q.read();
        let release = q.read();
        assert_eq!((press.x, press.y, press.buttons), (30, 98, 1));
        assert_eq!((release.x, release.y, release.buttons), (31, 98, 0));
        assert_eq!(q.read().buttons, 0);
    }
    #[test]
    fn overflow_is_bounded_and_cancels_partial_gesture() {
        let mut q = PointerQueue::new();
        for i in 0..=EDGES {
            q.handle(
                Event::MouseButton {
                    btn: super::super::event::MouseButton::Left,
                    down: i % 2 == 0,
                },
                256,
                160,
            );
        }
        assert!(q.len <= EDGES);
        let cancel = q.read();
        assert_eq!((cancel.x, cancel.y, cancel.buttons), (-1, -1, 0));
    }
    #[test]
    fn absolute_axes_preserve_each_other_and_clamp_to_tablet() {
        let mut m = MouseState {
            x: 0,
            y: 0,
            buttons: 0,
        };
        apply(&mut m, Event::MouseAbsX(120), 256, 160);
        apply(&mut m, Event::MouseAbsY(99), 256, 160);
        assert_eq!((m.x, m.y), (120, 99));
        apply(&mut m, Event::MouseAbsX(255), 256, 160);
        assert_eq!((m.x, m.y), (255, 99));
        apply(&mut m, Event::MouseAbsY(999), 256, 160);
        apply(&mut m, Event::MouseAbsX(-10), 256, 160);
        assert_eq!((m.x, m.y), (0, 159));
    }
    #[test]
    fn all_eight_buttons_have_distinct_userspace_bits() {
        use crate::drivers::input::MouseButton;
        let mut bits = 0;
        for button in [
            MouseButton::Left,
            MouseButton::Right,
            MouseButton::Middle,
            MouseButton::Other(3),
            MouseButton::Other(4),
            MouseButton::Other(5),
            MouseButton::Other(6),
            MouseButton::Other(7),
        ] {
            let bit = button_bit(button);
            assert_eq!(bits & bit, 0);
            assert_ne!(bit, 0);
            bits |= bit;
        }
        assert_eq!(bits, 255);
        assert_eq!(button_bit(MouseButton::Other(8)), 0);
    }
}
