//! Memory-management syscalls — `brk`, `mmap`, `munmap`, `mprotect`.
use crate::arch::regs;
use crate::mm::{pmm, vmm};
use crate::proc;
use onyx_core::errno::Errno;

/// Page-align `n` upward to a 4 KiB boundary. Returns `None` on overflow
/// (n within the last 4095 addresses of the 64-bit space) instead of
/// silently wrapping to 0.
pub fn page_align_up(n: u64) -> Option<u64> {
    n.checked_add(0xFFF).map(|m| m & !0xFFF)
}
/// User-VA range check (Bug #27, #28 fix). Returns true iff
/// [addr, addr+size) lies entirely inside [USER_BASE, USER_TOP) with no
/// arithmetic overflow. Used by sys_mmap / sys_munmap / sys_mprotect to
/// prevent a user process from mapping, unmapping, or re-protecting
/// kernel pages.
pub fn user_range_ok(addr: u64, size: u64) -> bool {
    match addr.checked_add(size) {
        Some(end) => addr >= regs::USER_BASE && end <= regs::USER_TOP,
        None => false,
    }
}
/// Validate and back a break change before publishing it. The injected page
/// operations keep the state transition testable without global PMM/process
/// state. Successfully mapped pages remain owned/accounted after later failure.
fn change_heap_break(
    heap_brk: &mut u64,
    addr: u64,
    mut map_page: impl FnMut(u64) -> Result<(), Errno>,
    mut unmap_pages: impl FnMut(u64, usize) -> Result<(), Errno>,
) -> Result<u64, Errno> {
    if !(regs::USER_HEAP_BASE..=regs::USER_HEAP_BASE + regs::USER_HEAP_SIZE).contains(&addr) {
        return Err(Errno::NoMem);
    }
    let cur = *heap_brk;
    if addr > cur {
        let mut va = page_align_up(cur).ok_or(Errno::Range)?;
        let end = page_align_up(addr).ok_or(Errno::Range)?;
        while va < end {
            map_page(va)?;
            va += 4096;
        }
    } else if addr < cur {
        // A live partial page must remain mapped. Align both endpoints so
        // shrinking within one page cannot subtract ceil(addr) from cur.
        let from = page_align_up(addr).ok_or(Errno::Range)?;
        let end = page_align_up(cur).ok_or(Errno::Range)?;
        if from < end {
            unmap_pages(from, (end - from) as usize)?;
        }
    }
    *heap_brk = addr;
    Ok(addr)
}

/// Map one absent heap page, preserving the global user-page budget.
/// # Safety
/// `root_pa` is the live current address space's root; the caller exclusively
/// owns heap growth and runs with interrupts disabled (PMM/VMM lock contract).
unsafe fn map_heap_page(root_pa: u64, va: u64) -> Result<(), Errno> {
    // SAFETY: root_pa is a live root per the caller. The frame is zeroed and
    // private until map_one_pub succeeds; no process/list locks are acquired.
    unsafe {
        if vmm::translate_user(root_pa, va) != 0 {
            return Ok(());
        }
        crate::proc::limits::user_page_take()?;
        let page_pa = match pmm::alloc_zero() {
            Ok(pa) => pa,
            Err(e) => {
                crate::proc::limits::user_pages_release(1);
                return Err(e);
            }
        };
        let flags =
            regs::PTE_V | regs::PTE_R | regs::PTE_W | regs::PTE_U | regs::PTE_A | regs::PTE_D;
        if let Err(e) = vmm::map_one_pub(root_pa, va, page_pa, flags, 0) {
            pmm::free(page_pa);
            crate::proc::limits::user_pages_release(1);
            return Err(e);
        }
        Ok(())
    }
}

/// # Safety
/// Call only from the syscall path: current process set, ACL checked, with
/// exclusive ownership of its heap/root. Mapping operations must not schedule.
pub unsafe fn sys_brk(addr: u64) -> i64 {
    // SAFETY: this short Proc borrow does not cross another current() accessor
    // or a context switch. PMM/VMM leaf locks and accounting only use the root
    // copied below; unmapped pages release their budget through vmm::unmap.
    unsafe {
        let p = proc::current();
        if addr == 0 {
            return p.heap_brk as i64;
        }
        let root_pa = p.root_pa;
        match change_heap_break(
            &mut p.heap_brk,
            addr,
            |va| map_heap_page(root_pa, va),
            |from, size| vmm::unmap(root_pa, from, size),
        ) {
            Ok(new_brk) => new_brk as i64,
            Err(e) => e.as_i64(),
        }
    }
}

#[cfg(test)]
mod tests;
