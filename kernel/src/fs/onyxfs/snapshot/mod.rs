//! Snapshot subsystem: packed archives, legacy readers and restartable rollback.
//!
//! `format` owns the on-disk record layout, `stream` the block-stream
//! reader/writer, `engine` creation and restore, `api` the kernel-facing
//! entry points that run under the root VFS lock, and `access` the quarantine
//! gate for volumes whose rollback failed mid-way.
pub(crate) mod access;
mod api;
mod engine;
mod format;
mod stream;

pub(super) use api::snapshot_recover;
pub use api::{snapshot_create, snapshot_list, snapshot_rollback};
