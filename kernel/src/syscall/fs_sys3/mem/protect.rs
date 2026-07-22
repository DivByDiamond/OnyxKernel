use crate::arch::regs;
use crate::mm::vmm;
use crate::proc;
use onyx_core::errno::Errno;

use super::brk::{page_align_up, user_range_ok};

pub unsafe fn sys_munmap(addr: u64, length: u64) -> i64 {
    if addr & 0xFFF != 0 || length == 0 {
        return Errno::Inval.as_i64();
    }
    let size = page_align_up(length.max(4096)) as usize;
    if !user_range_ok(addr, size as u64) {
        return Errno::Inval.as_i64();
    }
    let p = proc::current();
    match vmm::unmap(p.root_pa, addr, size) {
        Ok(()) => 0,
        Err(e) => e.as_i64(),
    }
}

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
    if prot_r != 0 {
        new_flags |= regs::PTE_R;
    }
    if prot_w != 0 {
        new_flags |= regs::PTE_W;
    }
    if prot_x != 0 {
        new_flags |= regs::PTE_X;
    }
    if new_flags & regs::PTE_R == 0 && new_flags & regs::PTE_X == 0 {
        new_flags |= regs::PTE_R;
    }

    let mut va = addr;
    let end = match addr.checked_add(size) {
        Some(e) => e,
        None => return Errno::Inval.as_i64(),
    };
    while va < end {
        let pa = vmm::translate_user(p.root_pa, va);
        if pa != 0 {
            match vmm::map_one_pub(p.root_pa, va, pa, regs::PTE_V | new_flags, 0) {
                Ok(()) => {}
                Err(e) => return e.as_i64(),
            }
        }
        va += 4096;
    }
    0
}
