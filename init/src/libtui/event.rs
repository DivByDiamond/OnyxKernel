//! TUI event types

/// Input events (keyboard, mouse)
#[derive(Clone, Copy, Debug)]
pub enum Event {
    KeyPress(u8),
    MouseMove { x: u16, y: u16 },
    MouseClick { x: u16, y: u16, button: u8 },
    None,
}
