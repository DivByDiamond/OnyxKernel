use super::{FDT_MAGIC, G_DTB, G_STRUCT, G_STRINGS, G_STRUCT_SIZE};
use super::reader::rd32;

pub unsafe fn init(dtb_pa: usize) -> bool {
    if dtb_pa == 0 {
        return false;
    }
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
