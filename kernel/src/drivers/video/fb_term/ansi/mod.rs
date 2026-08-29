//! ANSI/VT100 escape-sequence interpreter for the kernel console.
//!
//! Full-screen terminal programs (editors like oed, monitors like osysmon)
//! drive the console with escape sequences. The kernel's fb_term provides a
//! plain character grid; this module interprets the standard CSI/ESC
//! commands and applies them to the grid, giving real cursor addressing,
//! colors, erase operations and scroll regions — enough for nano/vim/btop
//! style UIs without any userspace framebuffer access.
//!
//! Module layout (responsibility split):
//! - [`state`]: terminal state — cursor position, SGR attributes, scroll
//!   region, saved cursor.
//! - [`parse`]: parser state machine — byte feed, CSI dispatch, SGR.
//! - [`render`]: pixel-level rendering — character drawing, erase
//!   operations, region scrolling and the software cursor block.
//!
//! Supported sequences:
//!   CSI n A/B/C/D  — cursor up/down/forward/back
//!   CSI n;m H / f  — cursor position (1-based)
//!   CSI n G        — cursor to column
//!   CSI n d        — cursor to row
//!   CSI s / u      — save/restore cursor
//!   CSI ?25 h/l    — show/hide cursor
//!   CSI n J        — erase display (0=below,1=above,2=all,3=all+scrollback)
//!   CSI n K        — erase line   (0=right,1=left,2=all)
//!   CSI n;m;r SGR  — colors/attributes (0 reset,7 reverse,
//!                    30-37/90-97 fg, 40-47/100-107 bg, 39/49 default)
//!   CSI r          — set scroll region (top;bottom)
//!   ESC 7 / 8      — save/restore cursor (DEC)
//!   ESC M          — reverse index (scroll down inside region)
//!   ESC D          — index (scroll up inside region)
//!   ESC E          — next line

pub(crate) mod parse;
pub(crate) mod render;
pub(crate) mod state;

pub use state::AnsiTerm;

/// Global console state used by the sys_write path.
pub(crate) static mut G_ANSI: AnsiTerm = AnsiTerm::new();

/// Write a byte through the ANSI interpreter.
pub fn console_putc(c: u8) {
    // SAFETY: G_ANSI is a kernel-lifetime static; SIE=0 prevents same-hart preemption (see crate::sync). Cross-hart concurrent console_putc IS possible (klog from trap contexts) and tolerated by convention: output may interleave, the static never reallocates, and no reference escapes this block.
    unsafe {
        let t = &raw mut G_ANSI;
        (*t).putc(c);
    }
}

/// Refresh the cursor after a console operation.
pub fn console_cursor() {
    // SAFETY: G_ANSI is a kernel-lifetime static; SIE=0 prevents same-hart preemption (see crate::sync); cross-hart interleaving is tolerated by convention (see console_putc); draw_cursor only repaints the console grid.
    unsafe {
        let t = &raw mut G_ANSI;
        (*t).draw_cursor();
    }
}
