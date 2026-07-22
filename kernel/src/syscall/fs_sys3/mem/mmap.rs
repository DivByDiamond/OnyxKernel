use crate::arch::regs;
use crate::fs::vfs::{self, FD_TOKEN_NONE};
use crate::mm::vmm;
use crate::proc;
use onyx_core::errno::Errno;

use super::brk::page_align_up;

/// Anonymous or devfs-backed mmap.
pub unsafe fn sys_mmap(
    addr: u64,
    length: u64,
    prot: u64,
    flags: u64,
    fd: u64,
    _offset: u64,
) -> i64 {
    if length == 0 {
        return Errno::Inval.as_i64();
    }

    let prot_r = prot & 1;
    let prot_w = (prot >> 1) & 1;
    let prot_x = (prot >> 2) & 1;
    let mut pte_flags = regs::PTE_U | regs::PTE_A | regs::PTE_D;
    if prot_r != 0 {
        pte_flags |= regs::PTE_R;
    }
    if prot_w != 0 {
        pte_flags |= regs::PTE_W;
    }
    if prot_x != 0 {
        pte_flags |= regs::PTE_X;
    }
    if pte_flags & regs::PTE_R == 0 && pte_flags & regs::PTE_X == 0 {
        pte_flags |= regs::PTE_R;
    }

    if fd != FD_TOKEN_NONE {
        let token = fd as vfs::FdToken;
        let idx = match vfs::fd_check(token) {
            Ok(i) => i,
            Err(e) => return e.as_i64(),
        };
        let f = vfs::fd_get(idx);
        if f.fs != vfs::Fs::Devfs {
            return Errno::NoSys.as_i64();
        }
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
                let _ = vmm::unmap(p.root_pa, addr, size);
                addr
            } else if vmm::translate_user(p.root_pa, addr) != 0 {
                let va = p.mmap_brk;
                p.mmap_brk = va.wrapping_add(size as u64);
                va
            } else {
                addr
            }
        };

        let mmap_base: u64 = 0x2000_0000;
        let end = match vaddr.checked_add(size as u64) {
            Some(e) => e,
            None => return Errno::NoMem.as_i64(),
        };
        if vaddr < mmap_base || end > regs::USER_TOP {
            return Errno::NoMem.as_i64();
        }

        match crate::fs::devfs::mmap(f.ino, vaddr, size as u64, pte_flags) {
            Ok(va) => va as i64,
            Err(e) => e.as_i64(),
        }
    } else {
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
                let _ = vmm::unmap(p.root_pa, addr, size);
                addr
            } else if vmm::translate_user(p.root_pa, addr) != 0 {
                let va = p.mmap_brk;
                p.mmap_brk = va.wrapping_add(size as u64);
                va
            } else {
                addr
            }
        };

        let mmap_base: u64 = 0x2000_0000;
        let end = match vaddr.checked_add(size as u64) {
            Some(e) => e,
            None => return Errno::NoMem.as_i64(),
        };
        if vaddr < mmap_base || end > regs::USER_TOP {
            return Errno::NoMem.as_i64();
        }
        match vmm::map_anon(p.root_pa, vaddr, size, pte_flags) {
            Ok(()) => vaddr as i64,
            Err(e) => e.as_i64(),
        }
    }
}
