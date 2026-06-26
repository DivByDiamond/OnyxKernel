use crate::mm::{pmm, vmm};
use core::ptr;
use onyx_core::errno::{Errno, KResult};
use onyx_core::formats::OnxSegment;
use crate::arch::regs::*;

pub unsafe fn map_segment_data(root_pa: u64, s: &OnxSegment, image: *const u8, compressed: bool) -> KResult<()> {
    let seg_flags = (s.flags as u64) | PTE_U | PTE_A | PTE_D;
    let mut va = s.vaddr;
    let end = s.vaddr + s.memsz;
    while va < end {
        let page_base = va & !0xFFF;
        if vmm::translate_user(root_pa, page_base) == 0 {
            let page_pa = pmm::alloc_zero()?;
            vmm::map_one_pub(root_pa, page_base, page_pa, seg_flags, 0)?;
        }
        va = (page_base + 4096).min(end);
    }

    if compressed && s.compressed_size > 0 {
        decompress_to_pages(root_pa, s, image)
    } else {
        copy_raw_to_pages(root_pa, s, image)
    }
}

unsafe fn decompress_to_pages(root_pa: u64, s: &OnxSegment, image: *const u8) -> KResult<()> {
    let src = image.add(s.offset as usize);
    let comp_end = s.compressed_size as usize;
    let file_end = s.vaddr + s.filesz;
    let mut in_off = 0usize;
    let mut out_va = s.vaddr;
    while in_off < comp_end && out_va < file_end {
        let tag = *src.add(in_off);
        in_off += 1;
        if tag & 0x80 != 0 {
            let count = ((tag & 0x7F) as usize) + 1;
            if in_off >= comp_end { return Err(Errno::Inval); }
            let val = *src.add(in_off);
            in_off += 1;
            let mut left = count.min((file_end - out_va) as usize);
            while left > 0 {
                let pb = out_va & !0xFFF;
                let paddr = vmm::translate(root_pa, pb);
                let poff = (out_va & 0xFFF) as usize;
                let chunk = left.min(4096 - poff);
                ptr::write_bytes((paddr as *mut u8).add(poff), val, chunk);
                out_va += chunk as u64;
                left -= chunk;
            }
        } else {
            let count = (tag as usize) + 1;
            let mut left = count.min((file_end - out_va) as usize);
            if in_off + left > comp_end { return Err(Errno::Inval); }
            while left > 0 {
                let pb = out_va & !0xFFF;
                let paddr = vmm::translate(root_pa, pb);
                let poff = (out_va & 0xFFF) as usize;
                let chunk = left.min(4096 - poff);
                ptr::copy_nonoverlapping(src.add(in_off), (paddr as *mut u8).add(poff), chunk);
                in_off += chunk;
                out_va += chunk as u64;
                left -= chunk;
            }
        }
    }
    Ok(())
}

unsafe fn copy_raw_to_pages(root_pa: u64, s: &OnxSegment, image: *const u8) -> KResult<()> {
    let mut va = s.vaddr;
    let end = s.vaddr + s.memsz;
    let mut file_pos: u64 = 0;
    while va < end {
        let page_base = va & !0xFFF;
        let existing = vmm::translate_user(root_pa, page_base);
        let page_end = page_base + 4096;
        let page_va_end = page_end.min(end);
        let copy_len = (page_va_end - va).min(s.filesz.saturating_sub(file_pos));
        if copy_len > 0 {
            let abs_off = s.offset as u64 + file_pos;
            let src = image.add(abs_off as usize);
            let dst = (existing as *mut u8).add((va & 0xFFF) as usize);
            ptr::copy_nonoverlapping(src, dst, copy_len as usize);
        }
        file_pos += copy_len;
        va = page_va_end;
    }
    Ok(())
}
