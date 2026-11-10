//! Kernel entry points, one per boot chain.
//!
//! * [`boot_mmode`] — rv64, booted by OnyxBoot in M-mode (no firmware).
//! * [`boot_smode`] — rv64 under real firmware (OpenSBI / OC2R).
//! * [`boot_32`] — rv32, booted by OnyxBoot in M-mode.

#[cfg(all(target_pointer_width = "32", not(feature = "smode")))]
pub mod boot_32;
#[cfg(all(target_pointer_width = "64", not(feature = "smode")))]
pub mod boot_mmode;
#[cfg(all(target_pointer_width = "64", feature = "smode"))]
pub mod boot_smode;
