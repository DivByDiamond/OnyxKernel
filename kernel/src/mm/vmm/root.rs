//! VMM — Sv39 paging with leaf-splitting.
//!
//! This is the directory root. It owns the kernel root page-table pointer
//! (`G_KERNEL_ROOT_PA`), the `new_root`/`install_root`/`init`/`kernel_root`
//! lifecycle helpers, `destroy_root` (with `free_subtree`), and the
//! `translate`/`translate_user` walkers. Map operations live in `map.rs`;
//! the page-table walker and leaf-splitting live in `walk.rs`.
use crate::arch::csr;
use crate::arch::regs::*;
use crate::mm::pmm;
use core::ptr;
use onyx_core::errno::KResult;

pub(super) static mut G_KERNEL_ROOT_PA: u64 = 0;

/// Allocate a zeroed page-table root.
///
/// # Safety
///
/// PMM must be initialised; interrupts disabled (spinlock invariant).
pub unsafe fn new_root() -> KResult<u64> {
    // SAFETY: `pmm::alloc_zero` returns a zeroed PMM-managed frame, which
    // is exactly a valid empty root table (all PTEs invalid).
    unsafe { pmm::alloc_zero() }
}

/// Switch the current hart's SATP to `root_pa` and flush all TLB entries.
///
/// # Safety
///
/// `root_pa` must be a valid, fully initialised root table for this hart;
/// switching while code running on this hart still relies on the previous
/// address space will fault — callers must ensure the switch point is
/// safe (e.g. inside `trap_return`/context switch with kernel mappings
/// identity-reachable).
pub unsafe fn install_root(root_pa: u64) {
    // SAFETY: CSR writes are inherently privileged but not memory-unsafe;
    // the contract above guarantees the target root is valid. sfence_vma_all
    // ensures no stale translations from the previous satp survive.
    unsafe {
        #[cfg(target_pointer_width = "64")]
        {
            csr::write_satp(crate::arch::regs::SATP_MODE_SV39 | (root_pa >> 12));
        }
        #[cfg(target_pointer_width = "32")]
        {
            let satp = crate::arch::bits::SATP_MODE_SV32 | ((root_pa >> 12) & 0x3FFFFF) as u32;
            csr::write_satp(satp as u64);
        }
        csr::sfence_vma_all();
    }
}

/// Create and install the global kernel address space: 1 GiB identity
/// leaf mappings covering the kernel's physical window, recorded in
/// `G_KERNEL_ROOT_PA` (and `arch::smp`), then loaded into SATP.
///
/// # Safety
///
/// Must run once per boot, single-threaded, before any hart relies on
/// virtual addressing. PMM initialised; interrupts disabled.
pub unsafe fn init() -> KResult<u64> {
    // SAFETY: root comes fresh from `pmm::alloc_zero` (valid, zeroed);
    // the volatile writes target slots 0..3 of that table; `G_KERNEL_ROOT_PA`
    // is written single-threaded here; `install_root` upholds its own
    // contract for this freshly built table.
    unsafe {
        let root_pa = new_root()?;
        crate::arch::smp::G_KERNEL_ROOT_PA = root_pa;
        let leaf_flags = PTE_V | PTE_R | PTE_W | PTE_X | PTE_A | PTE_D;
        #[cfg(target_pointer_width = "64")]
        {
            let root = root_pa as *mut u64;
            for i in 0..3u64 {
                let pa = i << 30;
                ptr::write_volatile(
                    root.add(i as usize),
                    PTE_V | leaf_flags | (pa >> 12 << PTE_PPN_SHIFT),
                );
            }
        }
        #[cfg(target_pointer_width = "32")]
        {
            // Root-cause fix (2026-09-12, found while live-testing the rv32
            // timer port — todo.md "rv32 mtrap"): this used to write a
            // SINGLE leaf at index 0, covering only VA/PA 0x0-0x3FFFFF.
            // QEMU's riscv `virt` machine places DRAM at a fixed
            // 0x80000000 regardless of XLEN (same physical map as the
            // rv64 board) — the kernel itself loads at 0x80200000+. Sv32's
            // top level indexes 4 MiB megapages by VPN[1] = PA >> 22, so
            // 0x80000000's index is 512, not 0: the very first identity-
            // mapped instruction fetch after `install_root` (this
            // kernel's own code) had nothing mapped there and faulted
            // immediately. rv32 had never actually booted past this
            // point — the bug was invisible because nothing before this
            // session ever ran an rv32 build in QEMU (compile+clippy only).
            //
            // Also note `root` above is typed `*mut u64` for the rv64
            // branch, but Sv32 PTEs are natively 32-bit words — reusing
            // that pointer with `.add(index)` for any index > 0 would
            // stride by 8 bytes instead of 4 and write into the wrong
            // slot entirely, so this branch uses its own `*mut u32` view.
            //
            // A second bug found alongside the first (same debug session):
            // an early version of this fix only covered a 512 MiB window
            // at the DRAM base, but the VERY NEXT instruction after
            // `install_root` switches SATP is `uart::putc(b'V')` back in
            // `early_init` — the UART's MMIO base (0x1000_0000 on QEMU
            // `virt`, well below DRAM) was left unmapped, so paging killed
            // the console immediately. A THIRD unmapped-address fault
            // turned up later still, once boot reached fw_cfg/ramfb probing
            // (stval=0x3000_0000, PCIe ECAM territory) — every additional
            // device probed was liable to touch yet another address outside
            // whatever ad-hoc window had been mapped so far.
            //
            // Rather than keep chasing individual addresses, this now maps
            // the EXACT SAME physical range as the rv64 branch above:
            // 0x0-0xC000_0000 (3 GiB, indices 0/1/2 there as 1 GiB
            // gigapages). Sv32 has no page size that large — its top-level
            // granularity is a 4 MiB megapage (VPN[1] = PA >> 22) — so the
            // equivalent coverage takes 768 consecutive megapages (768 x
            // 4 MiB = 3 GiB) instead of 3 gigapages, but the SET OF
            // PHYSICAL ADDRESSES made accessible is identical: every low
            // MMIO device this kernel probes at boot (CLINT @0x0200_0000,
            // PLIC @0x0C00_0000, UART/virtio @0x1000_0000+, PCIe ECAM
            // @0x3000_0000+) and all of DRAM (@0x8000_0000+) in one sweep,
            // with no further address-by-address guessing.
            const MEGAPAGE: u64 = 1 << 22; // Sv32 VPN[1] granularity (4 MiB)
            const NPAGES: u64 = 768; // 768 x 4 MiB = 3 GiB (matches rv64's 3 x 1 GiB)
            let root32 = root_pa as *mut u32;
            for i in 0..NPAGES {
                let pa = i * MEGAPAGE;
                let pte = (PTE_V | leaf_flags | (pa >> 12 << PTE_PPN_SHIFT)) as u32;
                ptr::write_volatile(root32.add(i as usize), pte);
            }
        }
        let p = &raw mut G_KERNEL_ROOT_PA;
        *p = root_pa;
        install_root(root_pa);
        Ok(root_pa)
    }
}

pub fn kernel_root() -> u64 {
    // SAFETY: plain u64 read of a value written once during boot init;
    // no aliasing `&mut` exists, so this cannot race with a write.
    unsafe { G_KERNEL_ROOT_PA }
}

/// Free every table and user page reachable from `root_pa`, then the root
/// itself, and flush all TLB entries.
///
/// # Safety
///
/// The address space must be dead: no hart may still run on (or translate
/// through) `root_pa`. Interrupts disabled; caller must not hold the VMM
/// lock already (this takes it).
pub unsafe fn destroy_root(root_pa: u64) {
    // SAFETY: VMM lock held so no concurrent mapper can resurrect PTEs;
    // `free_subtree` only dereferences table slots reachable from a valid
    // root and only frees PMM-managed pages (guarded by `is_managed`).
    unsafe {
        super::lock::vmm_lock();
        let root = root_pa as *mut u64;
        #[cfg(target_pointer_width = "64")]
        free_subtree(root, 2);
        #[cfg(target_pointer_width = "32")]
        free_subtree(root, 1);
        super::lock::vmm_unlock();
        pmm::free(root_pa);
        csr::sfence_vma_all();
    }
}

/// Recursively free the leaf pages of a table subtree (the tables
/// themselves are freed by the callers).
///
/// # Safety
///
/// Caller must hold the VMM lock. `table` must point at a live, mapped-in
/// page table of level `level` whose entries were produced by this VMM;
/// recursion depth is bounded by the paging levels.
unsafe fn free_subtree(table: *mut u64, level: u32) {
    // SAFETY: VMM lock held; `table.add(i)` with i < PTES_PER_TABLE is in
    // bounds for a page-table page, and child PAs dereferenced recursively
    // come from valid non-leaf PTEs guarded by `level > 0`.
    unsafe {
        #[cfg(target_pointer_width = "64")]
        let entries = SV39_PTES_PER_TABLE;
        #[cfg(target_pointer_width = "32")]
        let entries = crate::arch::bits::PTES_PER_TABLE;
        for i in 0..entries {
            let pte = ptr::read_volatile(table.add(i));
            if pte & PTE_V == 0 {
                continue;
            }
            let is_leaf = pte & PTE_LEAF != 0;
            let child_pa = (pte & PTE_PPN_MASK) >> PTE_PPN_SHIFT << 12;
            if is_leaf {
                // Root-cause fix (SMP crash, todo.md "Отдельный SMP-краш под
                // -smp 2"): every process root carries 3 top-level 1 GiB
                // identity leaf PTEs installed by onx::load (kernel::load_into)
                // to keep kernel code/data reachable after `csrw satp` — see
                // the comment in onx/segments.rs. Those leaves are
                // deliberately non-PTE_U placeholders, NOT per-process pages;
                // freeing them here returned live kernel physical memory
                // (including the page at DRAM base, 0x80000000) to the pmm
                // allocator on every single process exit, so the next
                // unrelated allocation (trivially reachable on a second hart
                // under real SMP concurrency) reused and clobbered it,
                // producing the "random" illegal-instruction/page-fault
                // crashes reported after process exit under -smp 2. Only
                // PTE_U leaves are genuinely owned, per-process pages; the
                // kernel identity placeholders must never be freed here.
                if pte & PTE_U != 0 && pmm::is_managed(child_pa) {
                    crate::proc::limits::user_pages_release(1);
                    pmm::free(child_pa);
                }
            } else if level > 0 {
                free_subtree(child_pa as *mut u64, level - 1);
                pmm::free(child_pa);
            }
        }
    }
}
