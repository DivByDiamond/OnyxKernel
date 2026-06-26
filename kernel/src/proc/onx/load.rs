use crate::arch::regs::*;
use crate::mm::{pmm, vmm};
use core::ptr;
use onyx_core::errno::{Errno, KResult};
use onyx_core::formats::{ONX_FLAGS_COMPRESSED, ONX_FLAGS_RING1};

use super::segments::map_segment_data;

pub struct OnxLoadResult {
    pub entry: u64,
    pub root_pa: u64,
    pub ustack: u64,
    pub heap_brk: u64,
    pub ring: u8,
}

pub unsafe fn load(image: *const u8, image_size: usize) -> KResult<OnxLoadResult> {
    if image_size < 24 { return Err(Errno::Inval); }
    let image_slice = core::slice::from_raw_parts(image, image_size);
    let hdr = onyx_core::formats::OnxHeader::from_bytes(image_slice).ok_or(Errno::Inval)?;
    let compressed = hdr.flags & ONX_FLAGS_COMPRESSED != 0;

    let root_pa = vmm::new_root()?;
    let root = root_pa as *mut u64;
    let leaf = PTE_V | PTE_R | PTE_W | PTE_X | PTE_A | PTE_D;
    for i in 0..3u64 {
        ptr::write_volatile(root.add(i as usize), PTE_V | leaf | ((i << 30) >> 12 << PTE_PPN_SHIFT));
    }

    for s in &hdr.segs {
        if s.vaddr < USER_BASE || s.vaddr >= USER_TOP { return Err(Errno::Range); }
        if s.filesz > s.memsz { return Err(Errno::Inval); }
        let data_end = if compressed && s.compressed_size > 0 { s.offset as u64 + s.compressed_size as u64 } else { s.offset as u64 + s.filesz };
        if data_end > image_size as u64 { return Err(Errno::Range); }
        map_segment_data(root_pa, s, image, compressed)?;
    }

    let ustack_top = USER_TOP;
    let ustack_bottom = ustack_top - (USER_STACK_PAGES as u64) * 4096;
    let mut va = ustack_bottom;
    while va < ustack_top {
        let page_pa = pmm::alloc_zero()?;
        vmm::map_one_pub(root_pa, va, page_pa, PTE_V | PTE_R | PTE_W | PTE_U | PTE_A | PTE_D, 0)?;
        va += 4096;
    }

    let heap_bottom = USER_HEAP_BASE;
    let heap_top = heap_bottom + (USER_HEAP_PAGES as u64) * 4096;
    let mut va = heap_bottom;
    while va < heap_top {
        let page_pa = pmm::alloc_zero()?;
        vmm::map_one_pub(root_pa, va, page_pa, PTE_V | PTE_R | PTE_W | PTE_U | PTE_A | PTE_D, 0)?;
        va += 4096;
    }

    let ring = if hdr.flags & ONX_FLAGS_RING1 != 0 { 1 } else { 2 };
    Ok(OnxLoadResult { entry: hdr.entry, root_pa, ustack: ustack_top - 16, heap_brk: heap_bottom, ring })
}
