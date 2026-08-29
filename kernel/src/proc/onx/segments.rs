use crate::arch::regs::*;
use crate::mm::{pmm, vmm};
use core::ptr;
use onyx_core::errno::{Errno, KResult};
use onyx_core::formats::OnxSegment;

/// # Safety
///
/// Caller contract: `root_pa` a valid root table owned by this loader;
/// `s` validated (ranges in [USER_BASE, USER_TOP), filesz <= memsz,
/// data_end <= image_size) by onx::load; `image` readable for those bytes.
pub unsafe fn map_segment_data(
    root_pa: u64,
    s: &OnxSegment,
    image: *const u8,
    compressed: bool,
) -> KResult<()> {
    // SAFETY: all page-table access goes through the owning root table
    // (fresh from onx::load, not shared yet); freshly allocated pages are
    // freed on every error path and file ranges were validated by the caller.
    unsafe {
        let seg_flags = (s.flags as u64) | PTE_U | PTE_A | PTE_D;
        let mut va = s.vaddr;
        let end = s.vaddr + s.memsz;
        while va < end {
            let page_base = va & !0xFFF;
            // `translate_user` returns the PA only if the page is already
            // mapped WITH PTE_U (i.e. a previously-mapped user page from an
            // earlier segment). The 3 1 GiB identity-mapped leaf PTEs set up
            // by `onx::load` do NOT have PTE_U, so translate_user returns 0
            // for them — which is exactly what we want: those pages must be
            // replaced with freshly-allocated user pages, not "upgraded".
            let existing_pa = vmm::translate_user(root_pa, page_base);
            if existing_pa == 0 {
                // Page not mapped as a user page — allocate a fresh zero page.
                crate::proc::limits::user_page_take()?;
                let page_pa = match pmm::alloc_zero() {
                    Ok(pa) => pa,
                    Err(e) => {
                        crate::proc::limits::user_pages_release(1);
                        return Err(e);
                    }
                };
                if let Err(e) = vmm::map_one_pub(root_pa, page_base, page_pa, seg_flags, 0) {
                    // Page never entered the table — release it here so both
                    // the physical page and its budget slot are reclaimed.
                    pmm::free(page_pa);
                    crate::proc::limits::user_pages_release(1);
                    return Err(e);
                }
            } else {
                // Page is already mapped as a user page (from a previous
                // segment that shares this page). Upgrade permissions: OR
                // the segment's flags into the existing PTE so that, e.g.,
                // a .data segment needing PTE_W gets it even if .rodata
                // already mapped the page as r--.
                let existing_flags = vmm::pte_user_flags(root_pa, page_base);
                let combined_flags = existing_flags | seg_flags;
                if combined_flags != existing_flags
                    && !vmm::update_user_pte(root_pa, page_base, seg_flags)
                {
                    return Err(Errno::Exist);
                }
            }
            va = (page_base + 4096).min(end);
        }

        if compressed && s.compressed_size > 0 {
            decompress_to_pages(root_pa, s, image)
        } else {
            copy_raw_to_pages(root_pa, s, image)
        }
    }
}

/// # Safety
///
/// Caller contract: `root_pa` owned by this loader and every page of
/// [s.vaddr, s.vaddr+filesz) already mapped (map_segment_data ran); `image`
/// readable for the segment's compressed range (validated in onx::load).
unsafe fn decompress_to_pages(root_pa: u64, s: &OnxSegment, image: *const u8) -> KResult<()> {
    // SAFETY: src stays within the caller-validated image buffer (bounds
    // enforced by the in_off/comp_end checks); destination PAs come from
    // vmm::translate on pages this function mapped above.
    unsafe {
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
                if in_off >= comp_end {
                    return Err(Errno::Inval);
                }
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
                if in_off + left > comp_end {
                    return Err(Errno::Inval);
                }
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
}

/// # Safety
///
/// Caller contract: same as decompress_to_pages; `root_pa` owned by this
/// loader with all segment pages already mapped; `image` readable for the
/// raw file range.
unsafe fn copy_raw_to_pages(root_pa: u64, s: &OnxSegment, image: *const u8) -> KResult<()> {
    // SAFETY: per-page translation of the just-mapped user pages yields
    // valid PA destinations; src stays within the caller-validated image
    // (copy_len bounded by page remainder and remaining filesz).
    unsafe {
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
}
