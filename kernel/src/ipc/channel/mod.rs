// channel/channel.rs keeps the IPC-channel impl next to its types/ringbuf peers.
#[allow(clippy::module_inception)]
pub mod channel;
pub mod ringbuf;
pub mod types;
pub use channel::*;
pub use ringbuf::*;
pub use types::*;
