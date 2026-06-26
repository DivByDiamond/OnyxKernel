use crate::mm::vmm;
use crate::proc;
use onyx_core::errno::Errno;

pub unsafe fn sys_brk(addr: u64) -> i64 {
    let p = proc::current();
    let cur = p.heap_brk;
    let heap_base = crate::arch::regs::USER_HEAP_BASE;
    let heap_end = heap_base + crate::arch::regs::USER_HEAP_SIZE;
    if addr == 0 {
        return cur as i64;
    }
    if addr < heap_base || addr > heap_end {
        return Errno::NoMem.as_i64();
    }
    p.heap_brk = addr;
    addr as i64
}

pub unsafe fn sys_mmap(addr: u64, length: u64, prot: u64, _flags: u64, _fd: u64, _offset: u64) -> i64 {
    let prot_r = prot & 1;
    let prot_w = (prot >> 1) & 1;
    let prot_x = (prot >> 2) & 1;
    let mut flags = crate::arch::regs::PTE_U | crate::arch::regs::PTE_A | crate::arch::regs::PTE_D;
    if prot_r != 0 { flags |= crate::arch::regs::PTE_R; }
    if prot_w != 0 { flags |= crate::arch::regs::PTE_W; }
    if prot_x != 0 { flags |= crate::arch::regs::PTE_X; }
    if flags & crate::arch::regs::PTE_R == 0 && flags & crate::arch::regs::PTE_X == 0 {
        flags |= crate::arch::regs::PTE_R;
    }
    let size = length.max(4096);
    let p = proc::current();
    let vaddr = if addr == 0 {
        let va = p.mmap_brk;
        p.mmap_brk = va.wrapping_add(size);
        va
    } else {
        if addr & 0xFFF != 0 {
            return Errno::Inval.as_i64();
        }
        addr
    };
    if vaddr < crate::arch::regs::USER_BASE || vaddr.wrapping_add(size) > crate::arch::regs::USER_HEAP_BASE {
        return Errno::NoMem.as_i64();
    }
    match vmm::map_anon(p.root_pa, vaddr, size as usize, flags) {
        Ok(()) => vaddr as i64,
        Err(e) => e.as_i64(),
    }
}

pub unsafe fn sys_munmap(addr: u64, length: u64) -> i64 {
    if addr & 0xFFF != 0 {
        return Errno::Inval.as_i64();
    }
    let size = length.max(4096) as usize;
    let p = proc::current();
    match vmm::unmap(p.root_pa, addr, size) {
        Ok(()) => 0,
        Err(e) => e.as_i64(),
    }
}
