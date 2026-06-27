//! Memory-management syscalls — `brk`, `mmap`, `munmap`, `mprotect`.
use crate::arch::regs;
use crate::mm::{pmm, vmm};
use crate::proc;
use onyx_core::errno::Errno;

/// Page-align `n` upward to a 4 KiB boundary.
const fn page_align_up(n: u64) -> u64 {
    (n + 0xFFF) & !0xFFF
}

/// Ensure that all pages in `[heap_brk, new_brk)` are mapped in the current
/// process's root page table. Used by `brk`/`sbrk` to grow the heap on demand
/// instead of pre-allocating the entire heap region at load time.
unsafe fn ensure_heap_pages(root_pa: u64, old_brk: u64, new_brk: u64) -> Result<(), Errno> {
    if new_brk <= old_brk {
        return Ok(());
    }
    let mut va = page_align_up(old_brk);
    let end = page_align_up(new_brk);
    let flags = regs::PTE_V | regs::PTE_R | regs::PTE_W | regs::PTE_U | regs::PTE_A | regs::PTE_D;
    while va < end {
        // Skip if already mapped (e.g. initial 16 heap pages from the loader).
        if vmm::translate_user(root_pa, va) == 0 {
            let page_pa = pmm::alloc_zero()?;
            vmm::map_one_pub(root_pa, va, page_pa, flags, 0)?;
        }
        va += 4096;
    }
    Ok(())
}

pub unsafe fn sys_brk(addr: u64) -> i64 {
    let p = proc::current();
    let cur = p.heap_brk;
    let heap_base = regs::USER_HEAP_BASE;
    let heap_end = heap_base + regs::USER_HEAP_SIZE;
    if addr == 0 {
        return cur as i64;
    }
    if addr < heap_base || addr > heap_end {
        return Errno::NoMem.as_i64();
    }
    if addr > cur {
        if let Err(e) = ensure_heap_pages(p.root_pa, cur, addr) {
            return e.as_i64();
        }
    }
    p.heap_brk = addr;
    addr as i64
}

pub unsafe fn sys_sbrk(incr: i64) -> i64 {
    let pid = proc::current_pid();
    let p = proc::by_pid(pid).unwrap();
    let cur = p.heap_brk;
    let heap_end = regs::USER_HEAP_BASE + regs::USER_HEAP_SIZE;
    let new_brk = (cur as i64 + incr) as u64;
    if new_brk < regs::USER_HEAP_BASE || new_brk > heap_end {
        return Errno::NoMem.as_i64();
    }
    if new_brk > cur {
        if let Err(e) = ensure_heap_pages(p.root_pa, cur, new_brk) {
            return e.as_i64();
        }
    }
    p.heap_brk = new_brk;
    cur as i64
}

/// Anonymous mmap. File-backed mappings are not yet supported — the `fd` and
/// `offset` parameters are accepted for ABI compatibility but ignored.
///
/// `prot` bits (matches POSIX low bits):
///   bit 0 = PROT_READ, bit 1 = PROT_WRITE, bit 2 = PROT_EXEC.
///
/// `flags` bits (subset of POSIX):
///   bit 0 = MAP_SHARED  (currently treated same as PRIVATE — no COW yet)
///   bit 1 = MAP_PRIVATE (default)
///   bit 4 = MAP_FIXED   (must map at exactly `addr`)
pub unsafe fn sys_mmap(addr: u64, length: u64, prot: u64, flags: u64, _fd: u64, _offset: u64) -> i64 {
    if length == 0 {
        return Errno::Inval.as_i64();
    }
    let prot_r = prot & 1;
    let prot_w = (prot >> 1) & 1;
    let prot_x = (prot >> 2) & 1;
    let mut pte_flags = regs::PTE_U | regs::PTE_A | regs::PTE_D;
    if prot_r != 0 { pte_flags |= regs::PTE_R; }
    if prot_w != 0 { pte_flags |= regs::PTE_W; }
    if prot_x != 0 { pte_flags |= regs::PTE_X; }
    // POSIX: PROT_NONE is allowed but we always need at least R for the
    // hardware not to fault on metadata reads; treat as R-only.
    if pte_flags & regs::PTE_R == 0 && pte_flags & regs::PTE_X == 0 {
        pte_flags |= regs::PTE_R;
    }

    // Round size up to a whole number of pages — critical for VMM invariants.
    let size = page_align_up(length.max(4096)) as usize;

    let map_fixed = (flags & 0x10) != 0;
    let p = proc::current();

    let vaddr = if addr == 0 {
        let va = p.mmap_brk;
        p.mmap_brk = va.wrapping_add(size as u64);
        va
    } else {
        if addr & 0xFFF != 0 {
            return Errno::Inval.as_i64();
        }
        if map_fixed {
            // Unmap any overlapping pages first to honor MAP_FIXED semantics.
            let _ = vmm::unmap(p.root_pa, addr, size);
            addr
        } else if vmm::translate_user(p.root_pa, addr) != 0 {
            // Caller hinted at an address that's already in use — fall back to
            // the bump allocator rather than silently overwriting mappings.
            let va = p.mmap_brk;
            p.mmap_brk = va.wrapping_add(size as u64);
            va
        } else {
            addr
        }
    };

    // mmap region is [0x2000_0000, USER_HEAP_BASE) so it can never collide
    // with the heap or stack.
    let mmap_base: u64 = 0x2000_0000;
    if vaddr < mmap_base || vaddr.wrapping_add(size as u64) > regs::USER_HEAP_BASE {
        return Errno::NoMem.as_i64();
    }
    match vmm::map_anon(p.root_pa, vaddr, size, pte_flags) {
        Ok(()) => vaddr as i64,
        Err(e) => e.as_i64(),
    }
}

pub unsafe fn sys_munmap(addr: u64, length: u64) -> i64 {
    if addr & 0xFFF != 0 || length == 0 {
        return Errno::Inval.as_i64();
    }
    let size = page_align_up(length.max(4096)) as usize;
    let p = proc::current();
    match vmm::unmap(p.root_pa, addr, size) {
        Ok(()) => 0,
        Err(e) => e.as_i64(),
    }
}

/// Change protections of existing pages. POSIX `mprotect(addr, len, prot)`.
/// Only present (already-mapped) pages are updated; gaps are skipped silently
/// (matching Linux's permissive behavior for non-overlapping subranges).
pub unsafe fn sys_mprotect(addr: u64, length: u64, prot: u64) -> i64 {
    if addr & 0xFFF != 0 || length == 0 {
        return Errno::Inval.as_i64();
    }
    let p = proc::current();
    let size = page_align_up(length) as u64;
    let prot_r = prot & 1;
    let prot_w = (prot >> 1) & 1;
    let prot_x = (prot >> 2) & 1;
    let mut new_flags = regs::PTE_U | regs::PTE_A | regs::PTE_D;
    if prot_r != 0 { new_flags |= regs::PTE_R; }
    if prot_w != 0 { new_flags |= regs::PTE_W; }
    if prot_x != 0 { new_flags |= regs::PTE_X; }
    if new_flags & regs::PTE_R == 0 && new_flags & regs::PTE_X == 0 {
        new_flags |= regs::PTE_R;
    }

    let mut va = addr;
    let end = addr + size;
    while va < end {
        let pa = vmm::translate_user(p.root_pa, va);
        if pa != 0 {
            // Remap with new permissions. We trust translate_user returns the
            // backing physical address; flags are derived from `prot` arg.
            match vmm::map_one_pub(
                p.root_pa,
                va,
                pa,
                regs::PTE_V | new_flags,
                0,
            ) {
                Ok(()) => {}
                Err(e) => return e.as_i64(),
            }
        }
        va += 4096;
    }
    0
}
