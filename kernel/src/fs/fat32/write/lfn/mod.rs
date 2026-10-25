//! LFN creation: derive a unique 8.3 alias, then write the `0x0F` chain plus
//! the short entry for a new file or directory.
mod entries;
mod name;
mod utf16;

pub(crate) use entries::create_long;
pub(crate) use name::needs_lfn;
