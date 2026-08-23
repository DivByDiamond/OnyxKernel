use onyx_core::parser::be32;

pub(crate) unsafe fn rd32(p: *const u8) -> u32 { unsafe {
    be32(core::slice::from_raw_parts(p, 4))
}}

pub(crate) unsafe fn rd64(p: *const u8) -> u64 { unsafe {
    (rd32(p) as u64) << 32 | rd64_lo(p)
}}

/// Read the base address from a `reg` property, honoring the address-cell
/// width:
///   - len >= 12: two address cells (64-bit address), size may be 1 or 2
///     cells. QEMU virt and OC2R/sedna both use 2 address cells, so the
///     address is the first 8 bytes.
///   - len == 8: one address cell (32-bit) + one size cell → first 4 bytes.
pub(crate) unsafe fn reg_base(data: &[u8]) -> u64 { unsafe {
    if data.len() >= 12 {
        rd64(data.as_ptr())
    } else if data.len() >= 4 {
        // len 4 and 8 both read only the first (address) cell.
        rd32(data.as_ptr()) as u64
    } else {
        0
    }
}}

pub(crate) unsafe fn rd64_lo(p: *const u8) -> u64 { unsafe {
    rd32(p.add(4)) as u64
}}

pub(crate) unsafe fn rd64_hi(p: *const u8) -> u32 { unsafe {
    rd32(p)
}}

pub(crate) unsafe fn cstr_at(offset: u32) -> &'static str { unsafe {
    let p = (super::G_STRINGS + offset as usize) as *const u8;
    let mut len = 0;
    while *p.add(len) != 0 {
        len += 1;
    }
    core::str::from_utf8(core::slice::from_raw_parts(p, len)).unwrap_or("")
}}

pub unsafe fn prop_name(name_off: u32) -> &'static str { unsafe {
    cstr_at(name_off)
}}
