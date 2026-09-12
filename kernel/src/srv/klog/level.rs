//! Log level type and the runtime-adjustable max-level filter.
use core::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Err = 0,
    Wrn = 1,
    Inf = 2,
    Dbg = 3,
}
impl Level {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Dbg => "DBG",
            Self::Inf => "INF",
            Self::Wrn => "WRN",
            Self::Err => "ERR",
        }
    }
}

/// Maximum level still printed. Defaults to `Info` (the historical
/// behaviour: info, warnings and errors go out, debug chatter does not).
static MAX_LEVEL: AtomicU8 = AtomicU8::new(Level::Inf as u8);

pub fn set_max_level(level: Level) {
    MAX_LEVEL.store(level as u8, Ordering::Relaxed);
}

#[inline]
pub fn enabled(level: Level) -> bool {
    (level as u8) <= MAX_LEVEL.load(Ordering::Relaxed)
}
