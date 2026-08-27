// TODO(dead-code): auth::shadow::shadow_core — shared auth/syscalls module, compiled per onyx_init bin;
// Verified 2026-08-27: all items are live (each is used by at least one onyx_init
// bin; per-bin dead_code/unused_imports warnings are unavoidable without a lib
// target). Revisit if onyx_init gains a shared [lib] target.
#![allow(dead_code, unused_imports)]

use crate::auth::SHADOW_PATH;
use crate::auth::crypto::{
    classify_password, format_shadow_field, generate_salt, hash_password, parse_shadow_field,
};
use onyx_core::crypto::HashScheme;

/// Result of checking a password against /etc/shadow.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum VerifyOutcome {
    /// Verified against the current iterated KDF — nothing to do.
    Ok,
    /// Verified but stored under the legacy single-round scheme; caller
    /// should opportunistically rehash when it has write access.
    OkLegacy,
    /// No match (or unreadable/malformed shadow) — always fail closed.
    Fail,
}

pub fn read_shadow_password(username: &[u8]) -> Result<[u8; 128], i64> {
    let mut path_buf = [0u8; 64];
    let n = SHADOW_PATH.len().min(63);
    path_buf[..n].copy_from_slice(&SHADOW_PATH[..n]);
    let fd = unsafe { crate::syscalls::open(path_buf.as_ptr(), 0, 0) };
    if fd < 0 {
        return Err(fd);
    }
    let mut buf = [0u8; 4096];
    let mut total = 0usize;
    loop {
        let n = unsafe {
            crate::syscalls::read(
                fd as u64,
                buf[total..].as_mut_ptr(),
                (buf.len() - total) as u64,
            )
        };
        if n <= 0 {
            break;
        }
        total += n as usize;
        if total >= buf.len() {
            break;
        }
    }
    unsafe { crate::syscalls::close(fd as u64) };

    let mut shadow_val = [0u8; 128];
    let data = &buf[..total];
    let mut pos = 0;
    while pos < data.len() {
        let line_end = match data[pos..].iter().position(|&b| b == b'\n') {
            Some(n) => pos + n,
            None => data.len(),
        };
        let line = &data[pos..line_end];
        pos = line_end + 1;

        let colon = match line.iter().position(|&b| b == b':') {
            Some(n) => n,
            None => continue,
        };
        let name = &line[..colon];
        let entry = &line[colon + 1..];

        if name.len() == username.len() && name == username {
            let n = entry.len().min(127);
            shadow_val[..n].copy_from_slice(&entry[..n]);
            return Ok(shadow_val);
        }
    }
    Err(-2)
}

/// Parse the raw shadow value into a NUL-trimmed byte slice.
fn stored_slice(stored: &[u8; 128]) -> &[u8] {
    let len = stored.iter().position(|&b| b == 0).unwrap_or(stored.len());
    &stored[..len]
}

/// Verify `username`/`password`, transparently accepting both the current
/// iterated `$5$` scheme and the legacy single-round scheme. Fail-closed:
/// any read error or malformed entry is `Fail`.
///
/// Migration design: both schemes share the `$5$salt$hash` layout, so the
/// scheme is determined by re-computation (`classify_password`) rather
/// than a format tag. Callers that learn `OkLegacy` and hold write access
/// (root session) rewrite the entry via `update_shadow_password`, which
/// stores an iterated-format hash.
pub fn verify_shadow_outcome(username: &[u8], password: &[u8]) -> VerifyOutcome {
    let stored = match read_shadow_password(username) {
        Ok(s) => s,
        Err(_) => return VerifyOutcome::Fail,
    };

    let field = match parse_shadow_field(stored_slice(&stored)) {
        Some(f) => f,
        None => return VerifyOutcome::Fail,
    };

    match classify_password(password, &field) {
        HashScheme::Iterated => VerifyOutcome::Ok,
        HashScheme::Legacy => VerifyOutcome::OkLegacy,
        HashScheme::Unknown => VerifyOutcome::Fail,
    }
}

/// Back-compat wrapper used by passwd/su: any successful verify counts.
pub fn verify_shadow_password(username: &[u8], password: &[u8]) -> bool {
    verify_shadow_outcome(username, password) != VerifyOutcome::Fail
}

pub(crate) fn format_shadow_entry(username: &[u8], password: &[u8]) -> ([u8; 128], usize) {
    let salt = generate_salt();
    let hash = hash_password(password, &salt);

    let mut buf = [0u8; 128];
    let mut pos = 0;

    for &b in username {
        if pos >= buf.len() {
            break;
        }
        buf[pos] = b;
        pos += 1;
    }
    if pos < buf.len() {
        buf[pos] = b':';
        pos += 1;
    }
    // "$5$<salt-hex>$<hash-hex>" — see onyx_core::crypto::kdf for the
    // documented deviation from crypt(3) sha256crypt.
    let field_len = format_shadow_field(&salt, &hash, &mut buf[pos..]);
    pos += field_len;
    (buf, pos)
}
