use onyx_core::parser::be32;

/// # Safety
///
/// `p` must point to at least 4 readable, initialized bytes (in mapped
/// memory); the caller must have bounds-checked the offset against the
/// containing structure (FDT blob / property data) before the call.
pub(crate) unsafe fn rd32(p: *const u8) -> u32 {
    // SAFETY: caller contract guarantees 4 readable bytes at `p`.
    unsafe { be32(core::slice::from_raw_parts(p, 4)) }
}

/// # Safety
///
/// `p` must point to at least 8 readable, initialized bytes in mapped
/// memory (bounds-checked by the caller against the FDT blob).
pub(crate) unsafe fn rd64(p: *const u8) -> u64 {
    // SAFETY: caller contract guarantees 8 readable bytes at `p`.
    unsafe { (rd32(p) as u64) << 32 | rd64_lo(p) }
}

/// Read the base address from a `reg` property, honoring the address-cell
/// width:
///   - len >= 12: two address cells (64-bit address), size may be 1 or 2
///     cells. QEMU virt and OC2R/sedna both use 2 address cells, so the
///     address is the first 8 bytes.
///   - len == 8: one address cell (32-bit) + one size cell → first 4 bytes.
///
/// # Safety
///
/// `data` must be a valid initialized byte slice (as produced by the FDT
/// walker); all reads are guarded by the `data.len()` checks below.
pub(crate) unsafe fn reg_base(data: &[u8]) -> u64 {
    unsafe {
        if data.len() >= 12 {
            // SAFETY: len >= 12, so the 8-byte read is in bounds.
            rd64(data.as_ptr())
        } else if data.len() >= 4 {
            // len 4 and 8 both read only the first (address) cell.
            // SAFETY: len >= 4, so the 4-byte read is in bounds.
            rd32(data.as_ptr()) as u64
        } else {
            0
        }
    }
}

/// # Safety
///
/// `p` must point to at least 8 readable bytes in mapped memory.
pub(crate) unsafe fn rd64_lo(p: *const u8) -> u64 {
    // SAFETY: caller contract guarantees 8 readable bytes at `p`.
    unsafe { rd32(p.add(4)) as u64 }
}

/// # Safety
///
/// `p` must point to at least 4 readable bytes in mapped memory.
pub(crate) unsafe fn rd64_hi(p: *const u8) -> u32 {
    // SAFETY: caller contract guarantees 4 readable bytes at `p`.
    unsafe { rd32(p) }
}

/// # Safety
///
/// `offset` must address a NUL-terminated string within the FDT strings
/// block (i.e. `init()` must have succeeded and the offset must come from
/// the parsed struct block, per the FDT spec).
///
/// Bounds-checking fix (todo P1 #7): the NUL scan is bounded by the strings
/// block (off_dt_strings + size_dt_strings, captured by init_from) and the
/// offset itself is rejected when it falls outside the block. A corrupt or
/// missing terminator now yields "" instead of reading past the strings
/// block.
pub(crate) unsafe fn cstr_at(offset: u32) -> &'static str {
    unsafe {
        // SAFETY: `init()` validated the DTB header (magic + block bounds);
        // the scan below is bounded by the strings block end, so the raw
        // reads stay inside the validated blob.
        let base = super::G_STRINGS;
        let size = super::G_STRINGS_SIZE;
        if base == 0 {
            return "";
        }
        let off = offset as usize;
        // Reject offsets outside the strings block outright.
        if size == 0 || off >= size {
            return "";
        }
        let p = (base + off) as *const u8;
        let end = (base + size) as *const u8;
        let mut len = 0usize;
        while p.add(len) < end && *p.add(len) != 0 {
            len += 1;
        }
        // No NUL before the block end — malformed strings block, degrade to
        // "" instead of running past it.
        if p.add(len) >= end {
            return "";
        }
        // SAFETY: p..p+len was just read successfully inside the block; UTF-8 is checked.
        core::str::from_utf8(core::slice::from_raw_parts(p, len)).unwrap_or("")
    }
}

/// # Safety
///
/// Same contract as `cstr_at`: `init()` must have succeeded and `name_off`
/// must come from a parsed FDT property header.
pub unsafe fn prop_name(name_off: u32) -> &'static str {
    // SAFETY: caller contract matches `cstr_at` (validated FDT state).
    unsafe { cstr_at(name_off) }
}
