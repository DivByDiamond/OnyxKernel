//! virtio-input event decode — translate raw events to `input::Event`.
use super::{push, VirtioInputEvent, G_IN, N_EVENTS};
use crate::drivers::input::{self, Event, KeyCode, MouseButton};
use crate::drivers::virtio::R_QUEUE_NOTIFY;
use core::ptr;

// Linux input event types (subset).
pub const EV_KEY: u16 = 0x01;
pub const EV_REL: u16 = 0x02;
pub const EV_ABS: u16 = 0x03;
pub const EV_SYN: u16 = 0x00;

// Linux mouse button codes.
const _BTN_LEFT: u16 = 0x110;
const _BTN_RIGHT: u16 = 0x111;
const _BTN_MIDDLE: u16 = 0x112;
const REL_X: u16 = 0x00;
const REL_Y: u16 = 0x01;

/// High-level event type for callers that don't care about specifics.
#[derive(Clone, Copy)]
pub enum EventType {
    Key(KeyCode, bool),
    MouseRel(i32, i32),
    MouseButton(MouseButton, bool),
    Syn,
}

/// Poll the virtio-input device for the next event. Returns `None` when
/// no event is available. Each call may recycle the consumed descriptor.
pub fn poll() -> Option<EventType> {
    unsafe {
        let used_idx = ptr::read_volatile(ptr::addr_of!((*G_IN.used).idx));
        if used_idx == G_IN.last_used {
            return None;
        }
        let slot = (G_IN.last_used as usize) % N_EVENTS;
        G_IN.last_used = used_idx;
        let elem = ptr::read_volatile(ptr::addr_of!((*G_IN.used).ring[slot]));
        let buf_idx = (elem.idx as usize) % N_EVENTS;
        let ev = ptr::read_volatile((G_IN.ev_buf as *const VirtioInputEvent).add(buf_idx));
        // Recycle the descriptor.
        push(buf_idx);
        let base = G_IN.base;
        crate::drivers::virtio::reg_w(base, R_QUEUE_NOTIFY, 0);
        Some(translate(ev))
    }
}

fn translate(ev: VirtioInputEvent) -> EventType {
    match ev.type_ {
        EV_KEY => {
            let kc = linux_to_keycode(ev.code);
            EventType::Key(kc, ev.value != 0)
        }
        EV_REL => {
            let dx = if ev.code == REL_X { ev.value as i32 } else { 0 };
            let dy = if ev.code == REL_Y { ev.value as i32 } else { 0 };
            EventType::MouseRel(dx, dy)
        }
        _ => EventType::Syn,
    }
}

/// Convert a Linux input keycode to our internal `KeyCode`.
fn linux_to_keycode(code: u16) -> KeyCode {
    match code {
        1 => KeyCode::Esc,
        28 => KeyCode::Enter,
        14 => KeyCode::Backspace,
        15 => KeyCode::Tab,
        57 => KeyCode::Space,
        103 => KeyCode::Up,
        105 => KeyCode::Left,
        106 => KeyCode::Right,
        108 => KeyCode::Down,
        42 => KeyCode::LeftShift,
        54 => KeyCode::RightShift,
        29 => KeyCode::LeftCtrl,
        97 => KeyCode::RightCtrl,
        // Letters and digits use the ASCII value directly.
        c if (2..=11).contains(&c) => KeyCode::Digit((c - 1) as u8),
        c if (16..=25).contains(&c) => KeyCode::Letter(b'q' + (c - 16) as u8),
        c if (30..=38).contains(&c) => KeyCode::Letter(b'a' + (c - 30) as u8),
        _ => KeyCode::Unknown,
    }
}

/// Convert a polled virtio event into the unified `input::Event` form.
pub fn poll_unified() -> Option<Event> {
    match poll()? {
        EventType::Key(kc, down) => Some(Event::Key { code: kc, down }),
        EventType::MouseRel(dx, dy) => Some(Event::MouseRel { dx, dy }),
        EventType::MouseButton(btn, down) => Some(Event::MouseButton { btn, down }),
        EventType::Syn => None,
    }
}
