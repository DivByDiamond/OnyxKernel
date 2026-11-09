//! Short-name (8.3) derivation for long names.
use onyx_core::errno::{Errno, KResult};

use super::super::super::scan_dir_entries;

/// True when storing `name` as an 8.3 alias alone would lose information:
/// too long, mixed case, more than one dot, or characters 8.3 cannot hold.
pub(crate) fn needs_lfn(name: &[u8]) -> bool {
    if name.is_empty() || name.len() > 255 {
        return true;
    }
    let (base, ext) = split_name(name);
    if base.is_empty() || base.len() > 8 || ext.len() > 3 {
        return true;
    }
    if name.iter().any(u8::is_ascii_lowercase) {
        return true;
    }
    name.iter().any(|&c| c != b'.' && !short_char(c))
}

/// Split at the last dot; no dot means an empty extension.
fn split_name(name: &[u8]) -> (&[u8], &[u8]) {
    match name.iter().rposition(|&c| c == b'.') {
        Some(i) if i > 0 => (&name[..i], &name[i + 1..]),
        _ => (name, &[]),
    }
}

/// Characters allowed in an 8.3 name (FAT spec: A-Z 0-9 and a small set of
/// punctuation; space is only allowed as padding, never inside a name).
fn short_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || b"$%'-_@~`!(){}^#&".contains(&c)
}

/// Build one 11-byte alias. `lossy` keeps the first six characters and adds
/// `~N`; otherwise the base/extension are used verbatim (already validated).
fn build_short(name: &[u8], lossy: bool, num: u32, out: &mut [u8; 11]) {
    out.fill(b' ');
    let (base, ext) = split_name(name);
    if !lossy {
        let n = base.len().min(8);
        for (i, &c) in base[..n].iter().enumerate() {
            out[i] = c.to_ascii_uppercase();
        }
        for (i, &c) in ext.iter().take(3).enumerate() {
            out[8 + i] = c.to_ascii_uppercase();
        }
        return;
    }
    let tail = if num < 10 { 1 } else { 2 };
    let keep = 8 - 1 - tail;
    let mut n = 0usize;
    for &c in base {
        if n == keep {
            break;
        }
        if short_char(c) {
            out[n] = c.to_ascii_uppercase();
            n += 1;
        }
    }
    while n < keep {
        out[n] = b'_';
        n += 1;
    }
    out[keep] = b'~';
    if tail == 1 {
        out[keep + 1] = b'0' + (num % 10) as u8;
    } else {
        out[keep + 1] = b'0' + (num / 10) as u8;
        out[keep + 2] = b'0' + (num % 10) as u8;
    }
    for (i, &c) in ext.iter().take(3).enumerate() {
        out[8 + i] = c.to_ascii_uppercase();
    }
}

/// Restore the dotted text form of a short name so the 8.3 matcher can compare it.
fn short_as_text(short: &[u8; 11], out: &mut [u8; 12]) -> usize {
    let mut len = 8;
    while len > 0 && short[len - 1] == b' ' {
        len -= 1;
    }
    out[..len].copy_from_slice(&short[..len]);
    let mut ext_len = 3;
    while ext_len > 0 && short[8 + ext_len - 1] == b' ' {
        ext_len -= 1;
    }
    if ext_len > 0 {
        out[len] = b'.';
        out[len + 1..len + 1 + ext_len].copy_from_slice(&short[8..8 + ext_len]);
        len += 1 + ext_len;
    }
    len
}

/// Find an 8.3 alias for `name` that no live entry in `dir_cluster` uses.
///
/// # Safety
///
/// Caller must not invoke FAT32 I/O concurrently from multiple harts;
/// `dir_cluster` must be a valid data cluster.
pub(super) unsafe fn unique_short(dir_cluster: u32, name: &[u8]) -> KResult<[u8; 11]> {
    // SAFETY: scan_dir_entries owns all raw access; buffers are local.
    unsafe {
        let mut candidate = [0u8; 11];
        let (base, ext) = split_name(name);
        let clean = !base.is_empty()
            && base.len() <= 8
            && ext.len() <= 3
            && name.iter().all(|&c| c == b'.' || short_char(c));
        for attempt in 0..=99u32 {
            let lossy = !clean || attempt > 0;
            // `~N` numbering starts at 1. A clean name is tried verbatim
            // first; a lossy one steps 1, 2, ... for every attempt.
            let num = if !lossy {
                0
            } else if clean {
                attempt
            } else {
                attempt + 1
            };
            build_short(name, lossy, num, &mut candidate);
            let mut text = [0u8; 12];
            let len = short_as_text(&candidate, &mut text);
            let mut cluster = 0u32;
            let mut size = 0u32;
            let mut is_dir = false;
            let mut buf = [0u8; 512];
            match scan_dir_entries(
                dir_cluster,
                &text[..len],
                &mut cluster,
                &mut size,
                &mut is_dir,
                &mut buf,
            ) {
                Err(Errno::NoEnt) => return Ok(candidate),
                Ok(()) => continue,
                Err(e) => return Err(e),
            }
        }
        Err(Errno::Exist)
    }
}

#[cfg(test)]
mod tests;
