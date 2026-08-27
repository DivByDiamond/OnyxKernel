//! Pre-rendered 8x16 glyph bitmaps for the PSF2 font generator.
//!
//! Data split by responsibility:
//! - [`digits`]: default glyph + digit shapes (0-9).
//! - [`upper`]: uppercase Latin letters (A-Z).
//! - [`lower`]: lowercase Latin letters (a-z).

mod digits;
mod lower;
mod upper;

pub use digits::{DIGITS, GLYPH_DEFAULT};
pub use lower::ALPHA_LOWER;
pub use upper::ALPHA_UPPER;
