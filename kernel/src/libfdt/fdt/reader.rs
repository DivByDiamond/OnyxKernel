use onyx_core::parser::be32;

pub(crate) unsafe fn rd32(p: *const u8) -> u32 {
    be32(core::slice::from_raw_parts(p, 4))
}

pub(crate) unsafe fn rd64(p: *const u8) -> u64 {
    (rd32(p) as u64) << 32 | rd64_lo(p)
}

pub(crate) unsafe fn rd64_lo(p: *const u8) -> u64 {
    rd32(p.add(4)) as u64
}

pub(crate) unsafe fn rd64_hi(p: *const u8) -> u32 {
    rd32(p)
}

pub(crate) unsafe fn cstr_at(offset: u32) -> &'static str {
    let p = (*(&raw const super::G_STRINGS) + offset as usize) as *const u8;
    let mut len = 0;
    while *p.add(len) != 0 {
        len += 1;
    }
    core::str::from_utf8(core::slice::from_raw_parts(p, len)).unwrap_or("")
}

pub unsafe fn prop_name(name_off: u32) -> &'static str {
    cstr_at(name_off)
}
