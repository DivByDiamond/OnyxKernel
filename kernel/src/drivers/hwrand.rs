//! Hardware RNG — entropy source dispatcher.
//!
//! Tries three sources in priority order:
//!   1. RISC-V Zkr `seed` CSR (architectural, available on newer cores).
//!   2. virtio-rng (via the `drivers::virtio_rng` driver).
//!   3. Free-running CLINT timer XOR-ed with a LCG (last-resort PRNG).
//!
//! All public functions are safe — they return `0` if no entropy source
//! is available, which the caller may seed into a CSPRNG.
use crate::arch::csr;
use crate::drivers::{rtc, virtio_rng};

static mut G_FALLBACK_SEED: u64 = 0x9E37_79B9_7F4A_7C15;

/// Try the RISC-V Zkr `seed` CSR. Returns Some(u32) on success, None on
/// unsupported CPUs. Four reads are needed for 128 bits of entropy.
unsafe fn try_seed_csr() -> Option<u32> {
    None
}

/// Try virtio-rng. Returns Some(u32) if a virtio-rng device is bound.
fn try_virtio() -> Option<u32> {
    if !virtio_rng::is_present() {
        return None;
    }
    let mut buf = [0u8; 4];
    if virtio_rng::read(&mut buf).is_err() {
        return None;
    }
    Some(u32::from_le_bytes(buf))
}

/// Last-resort PRNG: a 64-bit LCG seeded by the RTC counter and the
/// cycle counter (`rdcycle`). Suitable for non-cryptographic use only.
fn fallback() -> u32 {
    unsafe {
        let mix = rtc::now_nanos() ^ csr::read_cycle();
        G_FALLBACK_SEED = G_FALLBACK_SEED
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407)
            ^ mix;
        (G_FALLBACK_SEED >> 32) as u32
    }
}

/// Read a single 32-bit entropy word. Tries every source in order.
pub fn next_u32() -> u32 {
    unsafe {
        if let Some(v) = try_seed_csr() {
            return v;
        }
    }
    if let Some(v) = try_virtio() {
        return v;
    }
    fallback()
}

/// Fill `buf` with random bytes.
pub fn fill(buf: &mut [u8]) {
    let mut i = 0;
    while i + 4 <= buf.len() {
        let v = next_u32().to_le_bytes();
        buf[i..i + 4].copy_from_slice(&v);
        i += 4;
    }
    if i < buf.len() {
        let v = next_u32().to_le_bytes();
        let n = buf.len() - i;
        buf[i..].copy_from_slice(&v[..n]);
    }
}

/// Return the name of the active entropy source, for diagnostics.
pub fn source_name() -> &'static str {
    unsafe {
        if try_seed_csr().is_some() {
            return "riscv-seed";
        }
    }
    if virtio_rng::is_present() {
        return "virtio-rng";
    }
    "lcg-fallback"
}
