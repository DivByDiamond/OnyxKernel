//! Human input devices: unified event model (`event`), mouse cursor state
//! (`mouse`) and PS/2 driver.
pub mod event;
pub mod mouse;
pub mod ps2;

pub use event::*;
