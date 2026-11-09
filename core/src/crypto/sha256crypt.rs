//! SHA-256 crypt (`$5$`), byte-compatible with glibc / libxcrypt
//! `sha256crypt` (Ulrich Drepper's specification).
//!
//! Deviations from the historical Onyx scheme: this one uses the crypt-base64
//! alphabet (not hex), honours the `rounds=` tag and runs the full A/B/DP/DS
//! stretching loop, so hashes can be verified by any Unix `crypt(3)` and
//! `/etc/shadow` entries become portable.
//!
//! Everything here is `no_std` and allocation-free: the only per-call state is
//! the 32-byte P/S digests, exactly like the reference implementation.

use super::sha256::{Sha256State, sha256_final, sha256_init, sha256_update};

/// Rounds used when a setting does not name them (glibc default).
pub const DEFAULT_ROUNDS: u32 = 5_000;
/// Lower bound accepted in a `rounds=` tag.
pub const MIN_ROUNDS: u32 = 1_000;
/// Upper bound accepted in a `rounds=` tag.
pub const MAX_ROUNDS: u32 = 999_999_999;
/// Maximum salt length; longer salts are truncated, as in libxcrypt.
pub const MAX_SALT: usize = 16;
/// Length of the crypt-base64 hash tail.
pub const HASH_CHARS: usize = 43;

/// crypt-base64 alphabet: `.` `/` digits, uppercase, lowercase.
/// Crypt base64 alphabet (crypt(3) `./0-9A-Za-z`).
pub(super) const B64: &[u8; 64] = b"./0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
/// Scheme marker.
const SALT_PREFIX: &[u8] = b"$5$";
/// Optional rounds tag.
const ROUNDS_PREFIX: &[u8] = b"rounds=";

/// A parsed stored hash.
pub struct Setting<'a> {
    pub salt: &'a [u8],
    pub rounds: u32,
    /// The 43 plaintext hash characters (not decoded).
    pub hash: &'a [u8],
}

/// Feed `ctx` with `len` bytes of `block` repeated cyclically.
fn update_recycled(ctx: &mut Sha256State, block: &[u8; 32], len: usize) {
    let mut left = len;
    while left >= 32 {
        sha256_update(ctx, block);
        left -= 32;
    }
    sha256_update(ctx, &block[..left]);
}

/// Run the `$5$` stretching loop and return the final 32-byte digest.
pub fn digest(password: &[u8], salt: &[u8], rounds: u32) -> [u8; 32] {
    // B = SHA256(password || salt || password).
    let mut ctx = sha256_init();
    sha256_update(&mut ctx, password);
    sha256_update(&mut ctx, salt);
    sha256_update(&mut ctx, password);
    let alt = sha256_final(ctx);

    // A = SHA256(password || salt || B repeated over the password length,
    //            then one B or one password per bit of that length).
    let mut ctx = sha256_init();
    sha256_update(&mut ctx, password);
    sha256_update(&mut ctx, salt);
    update_recycled(&mut ctx, &alt, password.len());
    let mut bits = password.len();
    while bits > 0 {
        if bits & 1 != 0 {
            sha256_update(&mut ctx, &alt);
        } else {
            sha256_update(&mut ctx, password);
        }
        bits >>= 1;
    }
    let mut result = sha256_final(ctx);

    // P = SHA256(password repeated once per password byte).
    let mut ctx = sha256_init();
    for _ in 0..password.len() {
        sha256_update(&mut ctx, password);
    }
    let p_bytes = sha256_final(ctx);

    // S = SHA256(salt repeated 16 + A[0] times).
    let mut ctx = sha256_init();
    for _ in 0..16usize + usize::from(result[0]) {
        sha256_update(&mut ctx, salt);
    }
    let s_bytes = sha256_final(ctx);

    for round in 0..rounds {
        let odd = round & 1 != 0;
        let mut ctx = sha256_init();
        if odd {
            update_recycled(&mut ctx, &p_bytes, password.len());
        } else {
            sha256_update(&mut ctx, &result);
        }
        if round % 3 != 0 {
            update_recycled(&mut ctx, &s_bytes, salt.len());
        }
        if round % 7 != 0 {
            update_recycled(&mut ctx, &p_bytes, password.len());
        }
        if odd {
            sha256_update(&mut ctx, &result);
        } else {
            update_recycled(&mut ctx, &p_bytes, password.len());
        }
        result = sha256_final(ctx);
    }
    result
}

/// `b64_from_24bit` from the reference implementation: emit `n` base64
/// characters of the 24-bit word, least significant 6 bits first.
fn b64_from_24bit(b2: u8, b1: u8, b0: u8, n: usize, out: &mut [u8], pos: &mut usize) {
    let mut word = (u32::from(b2) << 16) | (u32::from(b1) << 8) | u32::from(b0);
    for _ in 0..n {
        out[*pos] = B64[(word & 0x3F) as usize];
        *pos += 1;
        word >>= 6;
    }
}

/// Encode the digest into the 43-character crypt-base64 tail.
pub fn encode(digest: &[u8; 32], out: &mut [u8]) -> Option<()> {
    if out.len() < HASH_CHARS {
        return None;
    }
    let mut pos = 0usize;
    let d = digest;
    b64_from_24bit(d[0], d[10], d[20], 4, out, &mut pos);
    b64_from_24bit(d[21], d[1], d[11], 4, out, &mut pos);
    b64_from_24bit(d[12], d[22], d[2], 4, out, &mut pos);
    b64_from_24bit(d[3], d[13], d[23], 4, out, &mut pos);
    b64_from_24bit(d[24], d[4], d[14], 4, out, &mut pos);
    b64_from_24bit(d[15], d[25], d[5], 4, out, &mut pos);
    b64_from_24bit(d[6], d[16], d[26], 4, out, &mut pos);
    b64_from_24bit(d[27], d[7], d[17], 4, out, &mut pos);
    b64_from_24bit(d[18], d[28], d[8], 4, out, &mut pos);
    b64_from_24bit(d[9], d[19], d[29], 4, out, &mut pos);
    b64_from_24bit(0, d[31], d[30], 3, out, &mut pos);
    Some(())
}

/// Parse a stored `$5$[rounds=N$]salt$hash` value.
/// Returns `None` for anything malformed (including the Onyx legacy hex
/// layout, whose hash tail is 64 characters long).
pub fn parse(stored: &[u8]) -> Option<Setting<'_>> {
    let mut rest = stored.strip_prefix(SALT_PREFIX)?;
    let mut rounds = DEFAULT_ROUNDS;
    if let Some(after) = rest.strip_prefix(ROUNDS_PREFIX) {
        let end = after.iter().position(|&b| b == b'$')?;
        let digits = &after[..end];
        // No leading zero, no zero rounds, no overflow past the maximum.
        if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) || digits[0] == b'0' {
            return None;
        }
        let mut value: u32 = 0;
        for &d in digits {
            value = value.checked_mul(10)?.checked_add(u32::from(d - b'0'))?;
        }
        if !(MIN_ROUNDS..=MAX_ROUNDS).contains(&value) {
            return None;
        }
        rounds = value;
        rest = &after[end + 1..];
    }
    let salt_end = rest.iter().position(|&b| b == b'$')?;
    let salt = &rest[..salt_end.min(MAX_SALT)];
    if salt.iter().any(|&b| b == b':' || b == b'\n') {
        return None;
    }
    let hash = &rest[salt_end + 1..];
    if hash.len() != HASH_CHARS || !hash.iter().all(|&b| B64.contains(&b)) {
        return None;
    }
    Some(Setting { salt, rounds, hash })
}

/// Constant-time verification of `password` against a stored `$5$` hash.
pub fn verify(password: &[u8], stored: &[u8]) -> bool {
    let Some(setting) = parse(stored) else {
        return false;
    };
    let mut result = digest(password, setting.salt, setting.rounds);
    let mut encoded = [0u8; HASH_CHARS];
    let ok = encode(&result, &mut encoded).is_some()
        && super::kdf::const_time_eq(&encoded, setting.hash);
    result.fill(0);
    encoded.fill(0);
    ok
}

/// Format a complete `$5$[rounds=N$]salt$hash` string into `out`.
///
/// `rounds == DEFAULT_ROUNDS` omits the tag, matching what `crypt_gensalt`
/// produces; non-default rounds are written explicitly so the hash stays
/// verifiable. Returns the number of bytes written.
pub fn hash_into(password: &[u8], salt: &[u8], rounds: u32, out: &mut [u8]) -> Option<usize> {
    if !(MIN_ROUNDS..=MAX_ROUNDS).contains(&rounds) {
        return None;
    }
    let salt = &salt[..salt.len().min(MAX_SALT)];
    if salt.iter().any(|&b| b == b':' || b == b'\n' || b == b'$') {
        return None;
    }
    let mut pos = 0usize;
    for &b in SALT_PREFIX {
        *out.get_mut(pos)? = b;
        pos += 1;
    }
    if rounds != DEFAULT_ROUNDS {
        for &b in ROUNDS_PREFIX {
            *out.get_mut(pos)? = b;
            pos += 1;
        }
        let mut digits = [0u8; 10];
        let mut count = 0usize;
        let mut value = rounds;
        while value > 0 {
            digits[count] = b'0' + (value % 10) as u8;
            value /= 10;
            count += 1;
        }
        for i in (0..count).rev() {
            *out.get_mut(pos)? = digits[i];
            pos += 1;
        }
        *out.get_mut(pos)? = b'$';
        pos += 1;
    }
    for &b in salt {
        *out.get_mut(pos)? = b;
        pos += 1;
    }
    *out.get_mut(pos)? = b'$';
    pos += 1;
    let mut result = digest(password, salt, rounds);
    let mut encoded = [0u8; HASH_CHARS];
    encode(&result, &mut encoded)?;
    out.get_mut(pos..pos + HASH_CHARS)?
        .copy_from_slice(&encoded);
    result.fill(0);
    encoded.fill(0);
    Some(pos + HASH_CHARS)
}

