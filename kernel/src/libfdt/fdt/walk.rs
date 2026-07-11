use super::reader::rd32;
use super::{FDT_BEGIN_NODE, FDT_END, FDT_END_NODE, FDT_NOP, FDT_PROP, G_STRUCT, G_STRUCT_SIZE};

pub unsafe fn walk(cb: &mut dyn FnMut(&str, &[(u32, &[u8])]) -> bool) {
    if *(&raw const G_STRUCT) == 0 {
        return;
    }
    let mut p = *(&raw const G_STRUCT) as *const u8;
    let end = (*(&raw const G_STRUCT) + *(&raw const G_STRUCT_SIZE)) as *const u8;
    let mut props: [(u32, &[u8]); 32] = [(0, &[]); 32];
    let mut prop_count = 0usize;
    let mut node_name: &str = "";

    while p < end {
        let tok = rd32(p);
        p = p.add(4);
        match tok {
            FDT_BEGIN_NODE => {
                let mut len = 0;
                while *p.add(len) != 0 {
                    len += 1;
                }
                node_name = core::str::from_utf8(core::slice::from_raw_parts(p, len)).unwrap_or("");
                p = p.add((len + 4) & !3);
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
                let prop_len = rd32(p) as usize;
                p = p.add(4);
                let name_off = rd32(p);
                p = p.add(4);
                let prop_data = core::slice::from_raw_parts(p, prop_len);
                if prop_count < 32 {
                    props[prop_count] = (name_off, prop_data);
                    prop_count += 1;
                }
                p = p.add((prop_len + 3) & !3);
            }
            FDT_NOP => {}
            FDT_END => return,
            _ => return,
        }
    }
}
