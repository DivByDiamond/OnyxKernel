//! SIGWINCH resize tracking (todo P2 #1).
//!
//! Whenever the framebuffer geometry changes (init, or a runtime resize on
//! hardware where the display can be reconfigured), the kernel flags the
//! event and notifies the foreground process. TIOCGWINSZ additionally
//! delivers one SIGWINCH on the first size read after a resize, so
//! TUI programs that only query the size see the change even if the
//! direct signal raced their handler installation.

use core::sync::atomic::{AtomicBool, Ordering};

static G_RESIZED: AtomicBool = AtomicBool::new(false);

/// Geometry-change hook: called by the fb driver when width/height are
/// (re)established. Flags the pending event and delivers SIGWINCH to the
/// foreground process (quiet no-op when no foreground process exists).
pub fn note_resized() {
    G_RESIZED.store(true, Ordering::Release);
    // SAFETY: signal_foreground validates the signal number and treats a
    // stale foreground pid as a no-op; no proc_list_lock is held here.
    unsafe {
        let _ = crate::proc::signal_foreground(crate::proc::SIGWINCH);
    }
}

/// TIOCGWINSZ hook: true exactly once after each geometry change. The
/// caller delivers SIGWINCH after filling in the new winsize.
pub fn take_resized() -> bool {
    G_RESIZED.swap(false, Ordering::AcqRel)
}
