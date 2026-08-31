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
    // SAFETY: G_MOUSE is kernel-owned static state; input dispatch runs in
    // kernel context with SIE clear (no same-hart preemption) and only one
    // dispatch site exists, so no interleaving occurs.
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
                let bit = match btn {
                    super::event::MouseButton::Left => BTN_LEFT,
                    super::event::MouseButton::Right => BTN_RIGHT,
                    super::event::MouseButton::Middle => BTN_MIDDLE,
                    super::event::MouseButton::Other(b) => {
                        if b > 0 {
                            0
                        } else {
                            return;
                        }
                    }
                };
                if down {
                    (*m).buttons |= bit;
                } else {
                    (*m).buttons &= !bit;
                }
            }
            _ => {}
        }
    }
}

/// Current pointer snapshot: (x, y, button bits) for SYS_mouse_read.
pub fn snapshot() -> (i16, i16, u8) {
    // SAFETY: plain read of kernel-owned static state (see handle()).
    unsafe {
        let m = &raw const G_MOUSE;
        ((*m).x as i16, (*m).y as i16, (*m).buttons)
    }
}
