//! Minimal TUI library for OnyxOS
//!
//! Provides basic widget system for terminal UI applications.

pub mod widget;
pub mod event;
pub mod layout;

pub use widget::{Widget, Button, Label, TextBox};
pub use event::Event;
pub use layout::Layout;
