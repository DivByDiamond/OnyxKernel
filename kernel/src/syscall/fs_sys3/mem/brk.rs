//! Memory-management syscalls — `brk`, `mmap`, `munmap`, `mprotect`.
use crate::arch::regs;
use crate::mm::{pmm, vmm};
use crate::proc;
use onyx_core::errno::Errno;

/// Page-align `n` upward to a 4 KiB boundary.
const fn page_align_up(n: u64) -> u64 {
    (n + 0xFFF) & !0xFFF
}
pub(super) use page_align_up;

/// User-VA range check (Bug #27, #28 fix). Returns true iff
/// [addr, addr+size) lies entirely inside [USER_BASE, USER_TOP) with no
/// arithmetic overflow. Used by sys_mmap / sys_munmap / sys_mprotect to
/// prevent a user process from mapping, unmapping, or re-protecting
/// kernel pages.
const fn user_range_ok(addr: u64, size: u64) -> bool {
    match addr.checked_add(size) {
        Some(end) => addr >= regs::USER_BASE && end <= regs::USER_TOP,
        None => false,
    }
}
pub(super) use user_range_ok;

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
    if addr < cur {
        let size = (cur - addr) as usize;
        let _ = vmm::unmap(p.root_pa, addr, size);
    }
    if addr > cur {
        if let Err(e) = ensure_heap_pages(p.root_pa, cur, addr) {
            return e.as_i64();
        }
    }
    p.heap_brk = addr;
    addr as i64
}

#[expect(dead_code)]
pub unsafe fn sys_sbrk(incr: i64) -> i64 {
    let pid = proc::current_pid();
    let p = match proc::by_pid(pid) {
        Some(proc) => proc,
        None => return Errno::Inval.as_i64(),
    };
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
