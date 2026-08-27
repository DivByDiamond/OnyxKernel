//! Entropy sources: hardware RNG (`hwrand`) and the non-cryptographic
//! boot-seeded PRNG (`prng`, xoshiro256**).
pub mod hwrand;
pub mod prng;

pub use prng::*;
