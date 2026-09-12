//! Pre-rendered 8x16 glyph bitmaps for the PSF2 font generator.
//!
//! Data split by responsibility:
//! - [`digits`]: default glyph + digit shapes (0-9).
//! - [`upper`]: uppercase Latin letters (A-Z).
//! - [`lower`]: lowercase Latin letters (a-z).
//! - [`punct`]: printable ASCII punctuation (everything else in 0x21-0x7E).

mod digits;
mod lower;
mod punct;
mod upper;

pub use digits::{DIGITS, GLYPH_DEFAULT};
pub use lower::ALPHA_LOWER;
pub use punct::punct_glyph;
pub use upper::ALPHA_UPPER;
