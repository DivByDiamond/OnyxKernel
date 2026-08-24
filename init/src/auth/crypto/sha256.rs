// TODO(dead-code): auth::crypto::sha256 — shared auth/syscalls module, compiled per onyx_init bin;
// items unused by one bin are used by others (dead_code/unused_imports fire per-bin).
#![allow(dead_code, unused_imports)]

// SHA-256 lives in onyx_core (pure, host-tested with RFC 6234 KATs) and is
// re-exported here so all init bins share one implementation (DRY).
pub use onyx_core::crypto::sha256::*;
