use super::reader::rd32;
use super::{
    FDT_BEGIN_NODE, FDT_END, FDT_END_NODE, FDT_NOP, FDT_PROP, G_STRUCT, G_STRUCT_SIZE, G_TOTALSIZE,
};

/// Walk the FDT struct block, invoking `cb` per node with its properties.
///
/// # Safety
///
/// `fdt::init()` must have succeeded (validating the magic) and must run
/// once during single-threaded boot; the global struct-block pointer/size
/// must still be untouched.
///
/// Bounds-checking fix (todo P1 #7): every token read, node-name scan and
/// property read is now bounded by the struct block end — and the block end
/// itself is additionally clamped to the DTB totalsize — so a truncated or
/// malformed blob can no longer be read past its buffer (the previous
/// implementation trusted the blob to be well-formed and could walk past
/// `end` on a truncated FDT_PROP or unterminated node name).
pub unsafe fn walk(cb: &mut dyn FnMut(&str, &[(u32, &[u8])]) -> bool) {
    unsafe {
        // SAFETY: caller contract: init() ran and validated the DTB, so
        // G_STRUCT/G_STRUCT_SIZE describe the magic-checked blob.
        if G_STRUCT == 0 {
            return;
        }
        let mut p = G_STRUCT as *const u8;
        // Clamp the walk end to the blob totalsize: G_STRUCT_SIZE comes from
        // the (validated) header, but defense in depth for a blob whose
        // struct block runs past totalsize.
        let block_end = G_STRUCT + G_STRUCT_SIZE;
        let end = if G_TOTALSIZE != 0 {
            let dtb_end = super::G_DTB + G_TOTALSIZE;
            if block_end > dtb_end {
                dtb_end
            } else {
                block_end
            }
        } else {
            block_end
        } as *const u8;
        let mut props: [(u32, &[u8]); 32] = [(0, &[]); 32];
        let mut prop_count = 0usize;
        let mut node_name: &str = "";

        // Token reads need 4 readable bytes; the loop guard enforces that
        // before every rd32.
        while p.wrapping_add(4) <= end {
            let tok = rd32(p);
            p = p.add(4);
            match tok {
                FDT_BEGIN_NODE => {
                    // Bounded NUL scan (fix): the name must terminate within
                    // the block; a malformed unterminated name aborts the
                    // walk instead of running off the blob.
                    let mut len = 0usize;
                    while p.add(len) < end && *p.add(len) != 0 {
                        len += 1;
                    }
                    if p.add(len) >= end {
                        // Unterminated node name — malformed blob, stop.
                        return;
                    }
                    node_name =
                        core::str::from_utf8(core::slice::from_raw_parts(p, len)).unwrap_or("");
                    // Advance past the NUL and round up to the next 4-byte
                    // boundary, clamped to the block end.
                    let next = p.add(len + 1);
                    let aligned = ((next as usize) + 3) & !3;
                    if aligned > end as usize {
                        return;
                    }
                    p = aligned as *const u8;
                    prop_count = 0;
                }
                FDT_END_NODE => {
                    if prop_count > 0 {
                        let slice = &props[..prop_count];
                        if cb(node_name, slice) {
                            return;
                        }
                    }
                }
                FDT_PROP => {
                    // The 8-byte FDT_PROP header (len, nameoff) must fit.
                    if p.add(8) > end {
                        return;
                    }
                    let prop_len = rd32(p) as usize;
                    p = p.add(4);
                    let name_off = rd32(p);
                    p = p.add(4);
                    // The property payload must fit inside the block.
                    if p.add(prop_len) > end {
                        return;
                    }
                    let prop_data = core::slice::from_raw_parts(p, prop_len);
                    if prop_count < 32 {
                        props[prop_count] = (name_off, prop_data);
                        prop_count += 1;
                    }
                    // Round the payload up to the next 4-byte boundary,
                    // clamped — a length that would push past `end` stops
                    // the walk here.
                    let aligned = ((p as usize) + prop_len + 3) & !3;
                    if aligned > end as usize {
                        return;
                    }
                    p = aligned as *const u8;
                }
                FDT_NOP => {}
                FDT_END => return,
                _ => return,
            }
        }
    }
}
