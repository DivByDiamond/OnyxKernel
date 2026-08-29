use crate::arch::regs;
use crate::fs::vfs::{self, FD_TOKEN_NONE};
use crate::mm::vmm;
use crate::proc;
use onyx_core::errno::Errno;

use super::brk::page_align_up;

const MMAP_BASE: u64 = 0x2000_0000;

/// Pick a virtual address for an anonymous/devfs mapping. The `mmap_brk`
/// hint is advanced ONLY after validation succeeds and the caller's map
/// operation returned Ok — otherwise a failed mmap would permanently burn
/// hint space (rollback bug).
/// # Safety
///
/// `p.root_pa` must satisfy the vmm translate contract (page-aligned PA of
/// a live, direct-mapped root table); `do_map` must map only into that same
/// table at the chosen `vaddr`.
unsafe fn resolve_vaddr(
    p: &mut crate::proc::process::Proc,
    addr: u64,
    size: u64,
    map_fixed: bool,
    do_map: impl FnOnce(u64) -> Result<(), Errno>,
) -> i64 {
    // SAFETY: p.root_pa satisfies the vmm translate contract per the
    // caller; unmap and translate_user operate only on that table, and the
    // chosen vaddr is checked to stay below USER_TOP before do_map runs.
    unsafe {
        let mut vaddr = addr;
        let advance_hint;
        if addr == 0 {
            vaddr = p.mmap_brk;
            advance_hint = true;
        } else {
            if addr & 0xFFF != 0 {
                return Errno::Inval.as_i64();
            }
            if map_fixed {
                // MAP_FIXED replaces whatever was mapped here before.
                let _ = vmm::unmap(p.root_pa, vaddr, size as usize);
                advance_hint = false;
            } else if vmm::translate_user(p.root_pa, addr) != 0 {
                // Requested range already occupied — fall back to the hint.
                vaddr = p.mmap_brk;
                advance_hint = true;
            } else {
                advance_hint = false;
            }
        }

        let end = match vaddr.checked_add(size) {
            Some(e) => e,
            None => return Errno::NoMem.as_i64(),
        };
        if vaddr < MMAP_BASE || end > regs::USER_TOP {
            return Errno::NoMem.as_i64();
        }

        match do_map(vaddr) {
            Ok(()) => {
                if advance_hint {
                    p.mmap_brk = vaddr + size;
                }
                vaddr as i64
            }
            Err(e) => e.as_i64(),
        }
    }
}

/// Anonymous or devfs-backed mmap.
/// # Safety
///
/// Call only from handler::handle's syscall path: current process set, ACL
/// checked; the fd path validates the token via vfs::fd_check before use.
pub unsafe fn sys_mmap(
    addr: u64,
    length: u64,
    prot: u64,
    flags: u64,
    fd: u64,
    _offset: u64,
) -> i64 {
    // SAFETY: p is this hart's current process and root_pa satisfies the
    // vmm translate contract; resolve_vaddr validates the vaddr and its
    // do_map callbacks map only into that same root table.
    unsafe {
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

        let size = match page_align_up(length.max(4096)) {
            Some(s) => s,
            None => return Errno::Range.as_i64(),
        };
        let map_fixed = (flags & 0x10) != 0;
        let p = proc::current();
        let root_pa = p.root_pa;

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
            resolve_vaddr(p, addr, size, map_fixed, |vaddr| {
                crate::fs::devfs::mmap(f.ino, vaddr, size, pte_flags).map(|_| ())
            })
        } else {
            resolve_vaddr(p, addr, size, map_fixed, |vaddr| {
                vmm::map_anon(root_pa, vaddr, size as usize, pte_flags)
            })
        }
    }
}
