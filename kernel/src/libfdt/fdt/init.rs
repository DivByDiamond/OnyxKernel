use super::reader::rd32;
use super::{FDT_MAGIC, G_DTB, G_STRINGS, G_STRUCT, G_STRUCT_SIZE};

fn scan_fdt_in_ram() -> Option<usize> {
    let ram_start = 0x8000_0000usize;
    let ram_end = 0x9000_0000usize;
    let scan_span = 0x0200_0000;
    let mut addr = ram_end - 4;
    let limit = ram_end.saturating_sub(scan_span);
    while addr > limit {
        let p = addr as *const u8;
        // SAFETY: `addr` is 4-byte aligned inside the scanned RAM window
        // (>= limit / < end), which is mapped RAM on both supported boards.
        if unsafe { rd32(p) } == FDT_MAGIC {
            return Some(addr);
        }
        addr = addr.wrapping_sub(4);
    }
    addr = ram_start;
    let end = ram_start + scan_span;
    while addr < end {
        let p = addr as *const u8;
        // SAFETY: `addr` is 4-byte aligned inside the scanned RAM window
        // (< end), which is mapped RAM on both supported boards.
        if unsafe { rd32(p) } == FDT_MAGIC {
            return Some(addr);
        }
        addr = addr.wrapping_add(4);
    }
    None
}

/// # Safety
///
/// `dtb_pa` must be the physical address of a candidate DTB in mapped RAM
/// (e.g. the `fdt_addr` handed to `kmain`). The header magic is verified
/// first; if it matches, the header fields at offsets 8/12/24 (FDT spec:
/// 40-byte header) are trusted and stored into the global state.
unsafe fn init_from(dtb_pa: usize) -> bool {
    unsafe {
        // SAFETY: dtb_pa points into mapped RAM per the caller contract;
        // the magic check below gates all further parsing.
        let hdr = dtb_pa as *const u8;
        let magic = rd32(hdr);
        if magic != FDT_MAGIC {
            crate::srv::klog::debug_mark(b'f');
            return false;
        }
        // SAFETY: header offsets 8..0x24 are inside the 40-byte FDT header;
        // the statics are written once during single-threaded early boot
        // (kmain, before secondary harts are started).
        let struct_off = rd32(hdr.add(4 * 2)) as usize;
        let strings_off = rd32(hdr.add(4 * 3)) as usize;
        // size_dt_struct is at offset 4*9 (0x24); 4*8 (0x20) is size_dt_strings.
        // Using the strings size as the struct bound truncated the FDT walk on
        // boards where size_dt_struct > size_dt_strings (e.g. sedna/OC2R), so
        // nodes past that offset (UART) were never found.
        let struct_size = rd32(hdr.add(4 * 9)) as usize;
        G_DTB = dtb_pa;
        G_STRUCT = dtb_pa + struct_off;
        G_STRINGS = dtb_pa + strings_off;
        G_STRUCT_SIZE = struct_size;
        true
    }
}

/// Parse the DTB at `dtb_pa` (or scan RAM for one). Must be called exactly
/// once, during single-threaded early boot, before any other libfdt API.
///
/// # Safety
///
/// `dtb_pa` must be a physical address in mapped RAM (0 for "scan only");
/// the scanned window is fixed RAM on supported boards.
pub unsafe fn init(dtb_pa: usize) -> bool {
    unsafe {
        // SAFETY: single-threaded boot-time call path (kmain/early_init);
        // dtb_pa is in mapped RAM per the caller contract.
        if dtb_pa != 0 && init_from(dtb_pa) {
            return true;
        }
        if let Some(found) = scan_fdt_in_ram() {
            // SAFETY: `found` matched FDT_MAGIC inside mapped RAM.
            return init_from(found);
        }
        false
    }
}
