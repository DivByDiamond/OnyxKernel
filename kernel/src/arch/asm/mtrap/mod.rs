//! M-mode trap shim used when the kernel is booted without firmware.
//!
//! [`mtrap`] is the rv64 shim, [`mtrap_32`] the rv32 one. Both service the
//! legacy SBI ecall contract and forward interrupts to S-mode. With real
//! firmware beneath the kernel (`smode`) neither is needed.

#[cfg(all(target_pointer_width = "64", not(feature = "smode")))]
mod mtrap;
#[cfg(all(target_pointer_width = "32", not(feature = "smode")))]
mod mtrap_32;

#[cfg(all(target_pointer_width = "64", not(feature = "smode")))]
pub use mtrap::G_MTRAP_SCRATCH;
#[cfg(all(target_pointer_width = "32", not(feature = "smode")))]
pub use mtrap_32::G_MTRAP_SCRATCH_32;
