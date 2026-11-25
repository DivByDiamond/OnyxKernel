//! Human input devices: unified event model (`event`), mouse cursor state
//! (`mouse`), kernel hotkeys (`hotkey`) and PS/2 driver.
pub mod event;
pub mod hotkey;
pub mod mouse;
pub mod ps2;
pub mod tty;

pub use event::*;
