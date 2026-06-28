use super::{FDT_MAGIC, G_DTB, G_STRUCT, G_STRINGS, G_STRUCT_SIZE};
use super::reader::rd32;

fn scan_fdt_in_ram() -> Option<usize> {
    let ram_start = 0x8000_0000usize;
    let ram_end = 0x9000_0000usize;
    let scan_span = 0x0200_0000;
    let mut addr = ram_end - 4;
    let limit = ram_end.saturating_sub(scan_span);
    while addr > limit {
        let p = addr as *const u8;
        if unsafe { rd32(p) } == FDT_MAGIC {
            return Some(addr);
        }
        addr = addr.wrapping_sub(4);
    }
    addr = ram_start;
    let end = ram_start + scan_span;
    while addr < end {
        let p = addr as *const u8;
        if unsafe { rd32(p) } == FDT_MAGIC {
            return Some(addr);
        }
        addr = addr.wrapping_add(4);
    }
    None
}

unsafe fn init_from(dtb_pa: usize) -> bool {
    let hdr = dtb_pa as *const u8;
    let magic = rd32(hdr);
    if magic != FDT_MAGIC {
        return false;
    }
    let struct_off = rd32(hdr.add(4 * 2)) as usize;
    let strings_off = rd32(hdr.add(4 * 3)) as usize;
    let struct_size = rd32(hdr.add(4 * 8)) as usize;
    *(&raw mut G_DTB) = dtb_pa;
    *(&raw mut G_STRUCT) = dtb_pa + struct_off;
    *(&raw mut G_STRINGS) = dtb_pa + strings_off;
    *(&raw mut G_STRUCT_SIZE) = struct_size;
    true
}

pub unsafe fn init(dtb_pa: usize) -> bool {
    if dtb_pa != 0 && init_from(dtb_pa) {
        return true;
    }
    if let Some(found) = scan_fdt_in_ram() {
        return init_from(found);
    }
    false
}
