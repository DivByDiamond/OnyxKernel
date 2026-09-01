//! Password hashing (KDF) and `/etc/shadow` field codecs for the Onyx
//! `$5$` scheme, plus the legacy pre-audit scheme used for transparent
//! migration. Pure functions — host-testable via `cargo test -p onyx_core`.
//!
//! FORMAT (documented deviation from crypt(3) `sha256crypt`):
//!   stored field = `$5$<salt as 16 lowercase hex>$<hash as 64 hex>`
//! We deliberately reuse the `$5$salt$hash` prefix layout so standard
//! tooling recognises the field shape and the salt stays visible, but the
//! digest is NOT sha256crypt: it is our fixed-iteration KDF below with
//! 10 000 rounds instead of the spec's variable `rounds=` parameter, and a
//! hex (not crypt base64) encoding. Entries produced this way will NOT
//! verify against glibc crypt(3).

use super::sha256::sha256;

/// Fixed iteration count of the current KDF.
pub const KDF_ITERS: usize = 10_000;

/// Salt size in bytes (stored as 16 hex chars).
pub const SALT_LEN: usize = 8;

/// Current KDF: `h = SHA256(pw); repeat 10_000x { h = SHA256(h || salt) }`.
pub fn hash_password(password: &[u8], salt: &[u8; SALT_LEN]) -> [u8; 32] {
    let mut h = sha256(password);
    let mut buf = [0u8; 40];
    buf[..32].copy_from_slice(&h);
    buf[32..].copy_from_slice(salt);
    for _ in 0..KDF_ITERS {
        h = sha256(&buf);
        buf[..32].copy_from_slice(&h);
    }
    h
}

/// Legacy pre-audit scheme (`$5$` prefix too): single-round
/// `SHA256(password || salt)` with no iteration. Kept ONLY so existing
/// images can log in once and be transparently rehashed to the iterated
/// format by the login path.
pub fn legacy_hash_password(password: &[u8], salt: &[u8; SALT_LEN]) -> [u8; 32] {
    let mut combined = [0u8; 72];
    let mut len = 0usize;
    for &b in password.iter().take(combined.len()) {
        combined[len] = b;
        len += 1;
    }
    for &b in salt.iter() {
        if len >= combined.len() {
            break;
        }
        combined[len] = b;
        len += 1;
    }
    sha256(&combined[..len])
}

/// Lowercase hex encode of up to 32 bytes into a fixed 64-byte buffer.
pub fn bytes_to_hex(bytes: &[u8]) -> [u8; 64] {
    let mut out = [0u8; 64];
    let hex_chars = b"0123456789abcdef";
    let n = bytes.len().min(32);
    for i in 0..n {
        out[i * 2] = hex_chars[(bytes[i] >> 4) as usize];
        out[i * 2 + 1] = hex_chars[(bytes[i] & 0xF) as usize];
    }
    out
}

fn hex_val(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        _ => 0,
    }
}

/// Decode exactly `SALT_LEN` bytes from 16 hex chars.
pub fn hex_decode_8(hex: &[u8]) -> [u8; SALT_LEN] {
    let mut out = [0u8; SALT_LEN];
    let n = (hex.len() / 2).min(SALT_LEN);
    for i in 0..n {
        out[i] = (hex_val(hex[i * 2]) << 4) | hex_val(hex[i * 2 + 1]);
    }
    out
}

/// Constant-time byte-slice comparison (early-outs only on length).
pub fn const_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut r = 0u8;
    for (ai, bi) in a.iter().zip(b.iter()) {
        r |= ai ^ bi;
    }
    r == 0
}

/// Parsed `$5$...` shadow password field.
#[derive(Clone, Copy)]
pub struct ShadowField {
    pub salt: [u8; SALT_LEN],
    /// Raw hash bytes decoded from the 64-char hex tail.
    pub hash: [u8; 32],
}

/// Parse a stored shadow password value. Accepts the shared `$5$salt$hash`
/// layout used by both the current iterated scheme and the legacy
/// single-round scheme (they are distinguished by re-computation, not by
/// format). Returns `None` for malformed/non-`$5$` fields.
///
/// Policy: an empty field, `*`, `!`, or any other non-`$5$` value is a
/// **locked account**, not "no password required". Unlike classic Unix
/// crypt(3) (empty hash historically meant login with any/no password),
/// Onyx never treats a missing/unparsable hash as an open door — callers
/// (`verify_shadow_outcome`) must see this as `None` so they fail closed.
/// An account that should log in with an empty password must have an
/// explicit iterated hash of the empty string (see `empty_password_verifies`
/// in kdf_tests.rs), not an empty shadow field.
pub fn parse_shadow_field(data: &[u8]) -> Option<ShadowField> {
    if data.len() < 3 + 17 + 64 || data[0] != b'$' || data[1] != b'5' || data[2] != b'$' {
        return None;
    }
    let rest = &data[3..];
    let salt_end = rest.iter().position(|&b| b == b'$')?;
    if salt_end != 16 {
        return None;
    }
    let salt_hex = &rest[..salt_end];
    let stored_hash_hex = rest.get(salt_end + 1..salt_end + 1 + 64)?;
    if !salt_hex
        .iter()
        .chain(stored_hash_hex.iter())
        .all(u8::is_ascii_hexdigit)
    {
        return None;
    }
    let mut hash = [0u8; 32];
    for i in 0..32 {
        hash[i] = (hex_val(stored_hash_hex[i * 2]) << 4) | hex_val(stored_hash_hex[i * 2 + 1]);
    }
    Some(ShadowField {
        salt: hex_decode_8(salt_hex),
        hash,
    })
}

/// Format `"$5$<salt-hex>$<hash-hex>"` into `buf`; returns written length.
/// `buf` must be at least 3 + 16 + 1 + 64 = 84 bytes.
pub fn format_shadow_field(salt: &[u8; SALT_LEN], hash: &[u8; 32], buf: &mut [u8]) -> usize {
    let salt_hex = bytes_to_hex(salt);
    let hash_hex = bytes_to_hex(hash);
    const PREFIX: &[u8] = b"$5$";
    let need = PREFIX.len() + 16 + 1 + 64;
    if buf.len() < need {
        return 0;
    }
    let mut pos = 0;
    buf[..PREFIX.len()].copy_from_slice(PREFIX);
    pos += PREFIX.len();
    buf[pos..pos + 16].copy_from_slice(&salt_hex[..16]);
    pos += 16;
    buf[pos] = b'$';
    pos += 1;
    buf[pos..pos + 64].copy_from_slice(&hash_hex);
    pos += 64;
    pos
}

/// Which scheme a stored field matches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HashScheme {
    /// Matches the current 10k-iterated KDF.
    Iterated,
    /// Matches the legacy single-round scheme (needs migration).
    Legacy,
    /// Matches neither (wrong password or corrupt entry).
    Unknown,
}

/// Classify a candidate password against a parsed shadow field by trying
/// both schemes. The iterated scheme is tried first so the common path
/// costs one extra SHA-256 only on failure.
pub fn classify_password(password: &[u8], field: &ShadowField) -> HashScheme {
    if const_time_eq(&hash_password(password, &field.salt), &field.hash) {
        return HashScheme::Iterated;
    }
    if const_time_eq(&legacy_hash_password(password, &field.salt), &field.hash) {
        return HashScheme::Legacy;
    }
    HashScheme::Unknown
}

#[cfg(test)]
#[path = "kdf_tests.rs"]
mod tests;
