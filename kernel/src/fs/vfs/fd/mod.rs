//! File-descriptor table and per-fd file I/O operations.
//!
//! `file`/`ops`/`table` hold the open-file entry points, allocation and raw
//! table plumbing; `io` carries the per-fd data path and `misc` the
//! descriptor plumbing (dup, stdio binding).
pub(crate) mod file;
pub(crate) mod io;
pub(crate) mod misc;
pub(crate) mod ops;
pub(crate) mod table;

pub(crate) use io::{fsync, rw, seek};
pub(crate) use misc::{dup, stdio};
