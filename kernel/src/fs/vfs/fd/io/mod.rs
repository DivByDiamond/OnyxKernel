//! Per-fd data-path operations: read/write, seek and flush.
pub(crate) mod fsync;
pub(crate) mod rw;
pub(crate) mod seek;
