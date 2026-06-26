//! xoshiro256** PRNG — fast, good statistical quality, NOT cryptographic.
//!
//! Seeded once at boot from `hwrand`. Suitable for non-deterministic
//! list iteration, hash-table salting, test-case fuzzing, ASLR.
//! For cryptographic use, prefer `hwrand::next_u32()` directly.
use crate::drivers::hwrand;

static mut G_STATE: [u64; 4] = [0; 4];
static mut G_SEEDED: bool = false;

#[inline]
fn rotl(x: u64, k: u32) -> u64 {
    x.rotate_left(k)
}

/// Seed the PRNG from the active hardware entropy source. Safe to call
/// multiple times — the state is replaced wholesale.
pub fn seed() {
    unsafe {
        for i in 0..4 {
            let a = hwrand::next_u32() as u64;
            let b = hwrand::next_u32() as u64;
            G_STATE[i] = (a << 32) | b;
        }
        // xoshiro cannot start from all-zero state.
        if G_STATE == [0, 0, 0, 0] {
            G_STATE = [0x9E37_79B9_7F4A_7C15, 0xD1B5_4A5D_6C8A_E963,
                       0xA409_3822_299F_31D0, 0x6550_0212_2AAA_C4F1];
        }
        G_SEEDED = true;
    }
}

/// Ensure the PRNG has been seeded. Called lazily by `next_u64`.
fn ensure_seeded() {
    unsafe {
        if !G_SEEDED {
            seed();
        }
    }
}

/// Generate the next 64-bit pseudo-random value.
pub fn next_u64() -> u64 {
    ensure_seeded();
    unsafe {
        let s = &raw mut G_STATE;
        let result = rotl((*s)[1].wrapping_mul(5), 7).wrapping_mul(9);
        let t = (*s)[1] << 17;
        (*s)[2] ^= (*s)[0];
        (*s)[3] ^= (*s)[1];
        (*s)[1] ^= (*s)[2];
        (*s)[0] ^= (*s)[3];
        (*s)[2] ^= t;
        (*s)[3] = rotl((*s)[3], 45);
        result
    }
}

/// Generate the next 32-bit pseudo-random value.
pub fn next_u32() -> u32 {
    (next_u64() & 0xFFFF_FFFF) as u32
}

/// Fill `buf` with pseudo-random bytes.
pub fn fill(buf: &mut [u8]) {
    ensure_seeded();
    let mut i = 0;
    while i + 8 <= buf.len() {
        let v = next_u64().to_le_bytes();
        buf[i..i + 8].copy_from_slice(&v);
        i += 8;
    }
    if i < buf.len() {
        let v = next_u64().to_le_bytes();
        let n = buf.len() - i;
        buf[i..].copy_from_slice(&v[..n]);
    }
}

/// Return a uniformly distributed pseudo-random value in `[0, bound)`.
pub fn below(bound: u32) -> u32 {
    if bound == 0 {
        return 0;
    }
    let r = bound;
    // Lemire's method: avoid modulo bias.
    let mut m = (next_u32() as u64) * (r as u64);
    let mut l = m as u32;
    if l < r {
        let t = (!r + 1) % r;
        while l < t {
            m = (next_u32() as u64) * (r as u64);
            l = m as u32;
        }
    }
    (m >> 32) as u32
}

/// Has the PRNG been seeded yet?
pub fn is_seeded() -> bool {
    unsafe { G_SEEDED }
}
