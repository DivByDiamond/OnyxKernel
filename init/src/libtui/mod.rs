//! Minimal TUI library for OnyxOS
//!
//! Provides basic widget system for terminal UI applications.

pub mod event;
pub mod layout;
pub mod widget;

pub use event::Event;
pub use layout::Layout;
pub use widget::{Button, Label, TextBox, Widget};
