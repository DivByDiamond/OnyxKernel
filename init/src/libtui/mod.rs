//! Minimal TUI library for OnyxOS
//!
//! Provides basic widget system for terminal UI applications: PSF font
//! loading/text blitting (font), widget set (widget), event types (event)
//! and a simple vertical layout (layout).

pub mod event;
pub mod render;
pub mod widget;

pub use event::Event;
pub use render::layout::Layout;
pub use widget::{Button, Label, TextBox, Widget};

// Preserve the pre-`render/` public path (`libtui::font`).
pub use render::font;
