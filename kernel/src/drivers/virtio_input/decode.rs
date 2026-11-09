//! virtio-input event decode — translate raw events to `input::Event`.
use super::{InDev, N_EVENTS, VirtioInputEvent, push};
use crate::drivers::input::{Event, KeyCode, MouseButton};
use crate::drivers::virtio::{R_QUEUE_NOTIFY, VIRTQ_SIZE, reg_w};
use core::ptr;

pub const EV_SYN: u16 = 0x00;
pub const EV_KEY: u16 = 0x01;
pub const EV_REL: u16 = 0x02;
pub const EV_ABS: u16 = 0x03;

const REL_X: u16 = 0x00;
const REL_Y: u16 = 0x01;
const BTN_LEFT: u16 = 0x110;
const BTN_RIGHT: u16 = 0x111;
const BTN_MIDDLE: u16 = 0x112;
const BTN_EXTRA: u16 = 0x114;
const BTN_SIDE: u16 = 0x113;

/// High-level event type for callers that don't care about specifics.
#[derive(Clone, Copy)]
pub enum EventType {
    Key(KeyCode, bool),
    MouseRel(i32, i32),
    MouseAbsX(i32),
    MouseAbsY(i32),
    MouseButton(MouseButton, bool),
    Syn,
}

/// Consume one event from one initialized queue. Caller holds INPUT_LOCK.
pub(super) unsafe fn poll_device(dev: &mut InDev) -> Option<VirtioInputEvent> {
    // SAFETY: ready implies all queue pages and the event buffer are live.
    unsafe {
        if !dev.ready {
            return None;
        }
        let used_idx = ptr::read_volatile(ptr::addr_of!((*dev.used).idx));
        if used_idx == dev.last_used {
            return None;
        }
        core::sync::atomic::fence(core::sync::atomic::Ordering::Acquire);
        let slot = (dev.last_used as usize) % VIRTQ_SIZE;
        dev.last_used = dev.last_used.wrapping_add(1);
        let elem = ptr::read_volatile(ptr::addr_of!((*dev.used).ring[slot]));
        if elem.idx as usize >= N_EVENTS {
            return Some(VirtioInputEvent {
                type_: EV_SYN,
                code: 0,
                value: 0,
            });
        }
        let index = elem.idx as usize;
        let event = if elem.len as usize >= core::mem::size_of::<VirtioInputEvent>() {
            ptr::read_volatile(dev.ev_buf.add(index))
        } else {
            VirtioInputEvent {
                type_: EV_SYN,
                code: 0,
                value: 0,
            }
        };
        push(dev, index);
        reg_w(dev.base, R_QUEUE_NOTIFY, 0);
        Some(event)
    }
}

pub(super) fn translate(ev: VirtioInputEvent) -> EventType {
    match ev.type_ {
        EV_KEY => {
            if ev.code >= BTN_LEFT {
                EventType::MouseButton(button(ev.code), ev.value != 0)
            } else {
                EventType::Key(linux_to_keycode(ev.code), ev.value != 0)
            }
        }
        EV_REL => {
            let dx = if ev.code == REL_X { ev.value as i32 } else { 0 };
            let dy = if ev.code == REL_Y { ev.value as i32 } else { 0 };
            EventType::MouseRel(dx, dy)
        }
        EV_ABS => match ev.code {
            REL_X => EventType::MouseAbsX(ev.value as i32),
            REL_Y => EventType::MouseAbsY(ev.value as i32),
            _ => EventType::Syn,
        },
        _ => EventType::Syn,
    }
}

fn button(code: u16) -> MouseButton {
    match code {
        BTN_LEFT => MouseButton::Left,
        BTN_RIGHT => MouseButton::Right,
        BTN_MIDDLE => MouseButton::Middle,
        BTN_SIDE => MouseButton::Other(3),
        BTN_EXTRA => MouseButton::Other(4),
        c => MouseButton::Other((c - BTN_LEFT) as u8),
    }
}

/// Convert a Linux input keycode to our internal `KeyCode`.
fn linux_to_keycode(code: u16) -> KeyCode {
    match code {
        1 => KeyCode::Esc,
        12 => KeyCode::Minus,
        13 => KeyCode::Equal,
        14 => KeyCode::Backspace,
        15 => KeyCode::Tab,
        26 => KeyCode::LeftBrace,
        27 => KeyCode::RightBrace,
        28 => KeyCode::Enter,
        29 => KeyCode::LeftCtrl,
        39 => KeyCode::Semicolon,
        40 => KeyCode::Apostrophe,
        41 => KeyCode::Grave,
        42 => KeyCode::LeftShift,
        43 => KeyCode::Backslash,
        51 => KeyCode::Comma,
        52 => KeyCode::Dot,
        53 => KeyCode::Slash,
        54 => KeyCode::RightShift,
        56 => KeyCode::LeftAlt,
        57 => KeyCode::Space,
        58 => KeyCode::CapsLock,
        69 => KeyCode::NumLock,
        70 => KeyCode::ScrollLock,
        97 => KeyCode::RightCtrl,
        100 => KeyCode::RightAlt,
        102 => KeyCode::Home,
        103 => KeyCode::Up,
        104 => KeyCode::PageUp,
        105 => KeyCode::Left,
        106 => KeyCode::Right,
        107 => KeyCode::End,
        108 => KeyCode::Down,
        109 => KeyCode::PageDown,
        110 => KeyCode::Insert,
        111 => KeyCode::Delete,
        c if (2..=11).contains(&c) => KeyCode::Digit(b"1234567890"[(c - 2) as usize]),
        c if (16..=25).contains(&c) => KeyCode::Letter(b"qwertyuiop"[(c - 16) as usize]),
        c if (30..=38).contains(&c) => KeyCode::Letter(b"asdfghjkl"[(c - 30) as usize]),
        c if (44..=50).contains(&c) => KeyCode::Letter(b"zxcvbnm"[(c - 44) as usize]),
        c if (59..=68).contains(&c) => KeyCode::F((c - 58) as u8),
        87 => KeyCode::F(11),
        88 => KeyCode::F(12),
        _ => KeyCode::Unknown,
    }
}

pub fn event_type(ev: EventType) -> Option<Event> {
    match ev {
        EventType::Key(kc, down) => Some(Event::Key { code: kc, down }),
        EventType::MouseRel(dx, dy) => Some(Event::MouseRel { dx, dy }),
        EventType::MouseAbsX(x) => Some(Event::MouseAbsX(x)),
        EventType::MouseAbsY(y) => Some(Event::MouseAbsY(y)),
        EventType::MouseButton(btn, down) => Some(Event::MouseButton { btn, down }),
        EventType::Syn => None,
    }
}

/// Convert a polled virtio event into the unified `input::Event` form.
pub fn poll_unified() -> Option<Event> {
    event_type(super::poll()?)
}

#[cfg(test)]
mod tests;
