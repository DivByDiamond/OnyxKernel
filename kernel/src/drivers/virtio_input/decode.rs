//! virtio-input event decode — translate raw events to `input::Event`.
use super::{G_IN, N_EVENTS, VirtioInputEvent, push};
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
    MouseAbs(i32, i32),
    MouseButton(MouseButton, bool),
    Syn,
}

/// Poll the virtio-input device for the next event. Returns `None` when
/// no event is available (including when no virtio-input device was ever
/// probed — `G_IN.base == 0` — since `G_IN.used` is a dangling pointer
/// until `init`/`setup_queue` run; without this guard the timer tick's
/// unconditional `input::event::poll_all()` call dereferences that
/// dangling pointer on every machine without a virtio-input device, which
/// only started actually happening once genuine timer interrupts began
/// firing — see `srv::timer::arm_timer`'s doc comment).
/// Each call consumes exactly one used-ring entry and recycles its descriptor.
pub fn poll() -> Option<EventType> {
    // SAFETY: valid only after init completed: used/ev_buf point at PMM rings/buffer;
    // slot is masked by VIRTQ_SIZE and buf_idx by N_EVENTS, both in-bounds;
    // reads are volatile of device-written memory.
    unsafe {
        if G_IN.base == 0 {
            return None;
        }
        let used_idx = ptr::read_volatile(ptr::addr_of!((*G_IN.used).idx));
        if used_idx == G_IN.last_used {
            return None;
        }
        let slot = (G_IN.last_used as usize) % VIRTQ_SIZE;
        G_IN.last_used = G_IN.last_used.wrapping_add(1);
        let elem = ptr::read_volatile(ptr::addr_of!((*G_IN.used).ring[slot]));
        let buf_idx = (elem.idx as usize) % N_EVENTS;
        let ev = ptr::read_volatile((G_IN.ev_buf as *const VirtioInputEvent).add(buf_idx));
        push(buf_idx);
        reg_w(G_IN.base, R_QUEUE_NOTIFY, 0);
        Some(translate(ev))
    }
}

fn translate(ev: VirtioInputEvent) -> EventType {
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
        EV_ABS => {
            let x = if ev.code == REL_X { ev.value as i32 } else { 0 };
            let y = if ev.code == REL_Y { ev.value as i32 } else { 0 };
            EventType::MouseAbs(x, y)
        }
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
        c if (2..=11).contains(&c) => KeyCode::Digit((c - 1) as u8),
        c if (16..=25).contains(&c) => KeyCode::Letter(b'q' + (c - 16) as u8),
        c if (30..=38).contains(&c) => KeyCode::Letter(b'a' + (c - 30) as u8),
        c if (44..=50).contains(&c) => KeyCode::Letter(b'z' + (c - 44) as u8),
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
        EventType::MouseAbs(x, y) => Some(Event::MouseAbs { x, y }),
        EventType::MouseButton(btn, down) => Some(Event::MouseButton { btn, down }),
        EventType::Syn => None,
    }
}

/// Convert a polled virtio event into the unified `input::Event` form.
pub fn poll_unified() -> Option<Event> {
    event_type(poll()?)
}
