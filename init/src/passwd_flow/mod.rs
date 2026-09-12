//! Helper logic for the `onyx-passwd` binary (entry point: `init/src/passwd.rs`).
//!
//! Split by responsibility to keep each file under the project's line-count
//! limit: error rendering, the ring-2 (self-service) flow, and the ring-1
//! (root/operator) flow.

pub mod errno;
pub mod root_flow;
pub mod user_flow;
