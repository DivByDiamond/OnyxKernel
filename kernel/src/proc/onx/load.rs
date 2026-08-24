use crate::arch::regs::*;
use crate::mm::{pmm, vmm};
use core::ptr;
use onyx_core::errno::{Errno, KResult};
use onyx_core::fmt::Arg;
use onyx_core::formats::{ONX_FLAGS_COMPRESSED, ONX_FLAGS_RING1, OnxHeader};

use super::segments::map_segment_data;

pub struct OnxLoadResult {
    pub entry: u64,
    pub root_pa: u64,
    pub ustack: u64,
    pub heap_brk: u64,
    pub ring: u8,
}

pub unsafe fn load(image: *const u8, image_size: usize) -> KResult<OnxLoadResult> {
    unsafe {
        if image_size < 24 {
            return Err(Errno::Inval);
        }
        let image_slice = core::slice::from_raw_parts(image, image_size);
        let hdr = OnxHeader::from_bytes(image_slice).ok_or(Errno::Inval)?;
        validate_image(&hdr)?;
        let compressed = hdr.flags & ONX_FLAGS_COMPRESSED != 0;

        let root_pa = vmm::new_root()?;
        match load_into(root_pa, &hdr, image, image_size, compressed) {
            Ok(r) => Ok(r),
            Err(e) => {
                // Tear down the half-built address space so its user pages
                // (and their proc::limits budget slots) are reclaimed.
                vmm::destroy_root(root_pa);
                Err(e)
            }
        }
    }
}

/// Validate an untrusted OnyxExec image before any mapping happens:
///
/// - every segment range [vaddr, vaddr+memsz) must lie inside
///   [USER_BASE, USER_TOP) — this also excludes page zero;
/// - entry must point inside the user range AND inside some loaded
///   segment (executable code lives in a LOAD segment).
///
/// A rejected image fails the exec cleanly with errno (the parent keeps
/// running); nothing has been mapped yet.
fn validate_image(hdr: &OnxHeader) -> KResult<()> {
    for s in &hdr.segs {
        let end = s.vaddr.checked_add(s.memsz).ok_or(Errno::Range)?;
        if s.vaddr < USER_BASE || end > USER_TOP {
            return Err(Errno::Range);
        }
    }
    if hdr.entry < USER_BASE || hdr.entry >= USER_TOP {
        return Err(Errno::Inval);
    }
    let entry_mapped = hdr
        .segs
        .iter()
        .any(|s| hdr.entry >= s.vaddr && hdr.entry < s.vaddr.saturating_add(s.memsz));
    if !entry_mapped {
        return Err(Errno::Inval);
    }
    Ok(())
}

/// Allocate + map one zeroed user page (stack / initial heap), accounting
/// it against the system-wide user-memory budget.
unsafe fn map_user_page(root_pa: u64, va: u64) -> KResult<()> {
    unsafe {
        crate::proc::limits::user_page_take()?;
        let page_pa = match pmm::alloc_zero() {
            Ok(pa) => pa,
            Err(e) => {
                crate::proc::limits::user_pages_release(1);
                return Err(e);
            }
        };
        if let Err(e) = vmm::map_one_pub(
            root_pa,
            va,
            page_pa,
            PTE_V | PTE_R | PTE_W | PTE_U | PTE_A | PTE_D,
            0,
        ) {
            // Page never entered the table — release it here so both the
            // physical page and its budget slot are reclaimed.
            pmm::free(page_pa);
            crate::proc::limits::user_pages_release(1);
            return Err(e);
        }
        Ok(())
    }
}

unsafe fn load_into(
    root_pa: u64,
    hdr: &OnxHeader,
    image: *const u8,
    image_size: usize,
    compressed: bool,
) -> KResult<OnxLoadResult> {
    unsafe {
        let root = root_pa as *mut u64;
        let leaf = PTE_V | PTE_R | PTE_W | PTE_X | PTE_A | PTE_D;
        for i in 0..3u64 {
            ptr::write_volatile(
                root.add(i as usize),
                PTE_V | leaf | ((i << 30) >> 12 << PTE_PPN_SHIFT),
            );
        }

        for s in &hdr.segs {
            crate::kinf!(
                "onx",
                "seg vaddr=%p memsz=%d filesz=%d flags=%d",
                Arg::from(s.vaddr),
                Arg::from(s.memsz),
                Arg::from(s.filesz),
                Arg::from(s.flags)
            );
            if s.vaddr < USER_BASE || s.vaddr >= USER_TOP {
                return Err(Errno::Range);
            }
            if s.filesz > s.memsz {
                return Err(Errno::Inval);
            }
            let data_end = if compressed && s.compressed_size > 0 {
                s.offset as u64 + s.compressed_size as u64
            } else {
                s.offset as u64 + s.filesz
            };
            if data_end > image_size as u64 {
                return Err(Errno::Range);
            }
            map_segment_data(root_pa, s, image, compressed)?;
        }

        let ustack_top = USER_TOP;
        let ustack_bottom = ustack_top - (USER_STACK_PAGES as u64) * 4096;
        let mut va = ustack_bottom;
        while va < ustack_top {
            map_user_page(root_pa, va)?;
            va += 4096;
        }

        let heap_bottom = USER_HEAP_BASE;
        let heap_top = heap_bottom + (USER_HEAP_PAGES as u64) * 4096;
        let mut va = heap_bottom;
        while va < heap_top {
            map_user_page(root_pa, va)?;
            va += 4096;
        }

        let ring = if hdr.flags & ONX_FLAGS_RING1 != 0 {
            1
        } else {
            2
        };
        Ok(OnxLoadResult {
            entry: hdr.entry,
            root_pa,
            ustack: ustack_top - 16,
            heap_brk: heap_bottom,
            ring,
        })
    }
}
