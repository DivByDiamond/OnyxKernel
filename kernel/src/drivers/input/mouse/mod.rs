//! Mouse cursor state (todo P3 #1): accumulates unified input events into
//! the single pointer snapshot SYS_mouse_read exposes to userland.
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

static mut G_MOUSE: MouseState = MouseState {
    x: 0,
    y: 0,
    buttons: 0,
};

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
        match ev {
            Event::MouseRel { dx, dy } => {
                (*m).x = clamp((*m).x + dx, w);
                (*m).y = clamp((*m).y + dy, h);
            }
            Event::MouseAbs { x, y } => {
                (*m).x = clamp(x, w);
                (*m).y = clamp(y, h);
            }
            Event::MouseButton { btn, down } => {
                let bit = button_bit(btn);
                if down {
                    (*m).buttons |= bit;
                } else {
                    (*m).buttons &= !bit;
                }
            }
            _ => {}
        }
    }
    MOUSE_LOCK.unlock();
}

/// Current pointer snapshot: (x, y, button bits) for SYS_mouse_read.
pub fn snapshot() -> (i16, i16, u8) {
    MOUSE_LOCK.lock();
    // SAFETY: lock excludes all mutations by input dispatch.
    let value = unsafe {
        let m = &raw const G_MOUSE;
        ((*m).x as i16, (*m).y as i16, (*m).buttons)
    };
    MOUSE_LOCK.unlock();
    value
}

#[cfg(test)]
mod tests {
    use super::*;
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
