// SHA-256 lives in onyx_core (pure, host-tested with RFC 6234 KATs) and is
// re-exported here so all init bins share one implementation (DRY).
pub use onyx_core::crypto::sha256::*;
