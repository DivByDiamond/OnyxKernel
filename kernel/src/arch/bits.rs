//! Architecture-specific constants gated by target_pointer_width.
//!
//! OnyxKernel was originally written for RISC-V 64-bit (Sv39 paging,
//! u64 pointers). This module adds cfg-gated support for RISC-V 32-bit
//! (Sv32 paging, u32 pointers) so the same source can be compiled for
//! both targets.
//!
//! ## What changes between 32-bit and 64-bit
//!
//! ### Paging
//! - Sv39 (64-bit): 3 levels, 512 entries/table, 4 KiB pages, 39-bit VA
//! - Sv32 (32-bit): 2 levels, 1024 entries/table, 4 KiB pages, 32-bit VA
//!
//! Sv32 has only 2 levels (vs Sv39's 3), so the page-table walker is
//! shorter. The PTE format also differs:
//! - Sv39 PTE: [ppn(44) | rsw(2) | daguxwr(8) | v(1) | reserved(9)]
//! - Sv32 PTE: [ppn(20) | rsw(2) | daguxwr(8) | v(1) | reserved(1)]
//!
//! ### CSR widths
//! - 64-bit: satp/scause/stval/sstatus are 64-bit
//! - 32-bit: satp/scause/stval/sstatus are 32-bit
//!
//! ### SATP mode field
//! - Sv39: bits 60-63 = 0x8
//! - Sv32: bit 31 = 0x1
//!
//! ### User VA range
//! - Sv39: USER_TOP = 0x4000_0000 (1 GiB user VA)
//! - Sv32: USER_TOP = 0x8000_0000 (2 GiB user VA, but realistically
//!   capped lower because the kernel needs the upper 2 GiB)
//!
//! ## Usage
//!
//! Code that needs to be pointer-width-aware should use the types and
//! constants from this module instead of hardcoding u64 / Sv39 values.
//! For example:
//!
//! ```ignore
//! use crate::arch::bits::*;
//! let pte: PteVal = PTE_V | PTE_R | ((pa >> 12) << PTE_PPN_SHIFT);
//! ```
//!
//! ## 32-bit port status — COMPLETE
//!
//! All architecture-dependent subsystems have been ported:
//!   ✅ boot_32.rs — rv32 boot entry with sw/lw, 32-bit PMP, BSS clear, mret
//!   ✅ trap_asm_32.rs — rv32 trap entry/return/sched_switch/drop_to_user
//!   ✅ walk_32.rs — Sv32 page-table walker with split_leaf (1024 entries)
//!   ✅ trap_frame.rs — 32-bit TrapFrame (u32 registers, 144 bytes)
//!   ✅ vmm/mod.rs — Sv32 init, install_root, translate, free_subtree
//!   ✅ map.rs/unmap.rs — Sv32 chunk sizes, alignment, walk selection
//!   ✅ smp.rs — Sv32 SATP encoding for secondary harts
//!   ✅ csr.rs — u32 CSR operands on rv32
//!   ✅ spawn.rs, fs_sys2.rs, extra.rs — Sv32 SATP in exec/fork
//!
//! Build: `cargo kbuild32` (requires riscv32imac-unknown-none-elf target).

#![allow(non_upper_case_globals)]

/// The pointer-sized unsigned integer for the target.
/// On 64-bit: u64. On 32-bit: u32.
#[cfg(target_pointer_width = "64")]
pub type usize_val = u64;
#[cfg(target_pointer_width = "32")]
pub type usize_val = u32;

/// PTE value type. Always usize_val — matches the platform's register width.
#[cfg(target_pointer_width = "64")]
pub type PteVal = u64;
#[cfg(target_pointer_width = "32")]
pub type PteVal = u32;

/// VA type. Always usize_val.
pub type Va = usize_val;
/// PA type. Always usize_val.
pub type Pa = usize_val;

// ── SATP mode ───────────────────────────────────────────────────────────

/// SATP mode field for Sv39 (64-bit only): bits 60-63 = 0x8.
#[cfg(target_pointer_width = "64")]
pub const SATP_MODE_SV39: u64 = 8 << 60;

/// SATP mode field for Sv32 (32-bit only): bit 31 = 0x1.
pub const SATP_MODE_SV32: u32 = 1 << 31;

/// SATP mode for the current target (Sv39 on 64-bit, Sv32 on 32-bit).
#[cfg(target_pointer_width = "64")]
pub const SATP_MODE_PAGING: PteVal = SATP_MODE_SV39;
#[cfg(target_pointer_width = "32")]
pub const SATP_MODE_PAGING: PteVal = SATP_MODE_SV32;

// ── PTE flags (identical layout for Sv32 and Sv39) ─────────────────────

pub const PTE_V: PteVal = 1;
pub const PTE_R: PteVal = 2;
pub const PTE_W: PteVal = 4;
pub const PTE_X: PteVal = 8;
pub const PTE_U: PteVal = 16;
pub const PTE_G: PteVal = 32;
pub const PTE_A: PteVal = 64;
pub const PTE_D: PteVal = 128;
pub const PTE_LEAF: PteVal = PTE_R | PTE_X;

/// PTE PPN shift. Same for Sv32 and Sv39 (10 = log2(4096) - 2 for the
/// 2 flag bits at the bottom that are always 0 in a leaf PTE's address).
pub const PTE_PPN_SHIFT: PteVal = 10;

/// PTE PPN mask.
/// - Sv39: 44-bit PPN, so mask = ((1<<44)-1) << 10
/// - Sv32: 20-bit PPN, so mask = ((1<<20)-1) << 10
#[cfg(target_pointer_width = "64")]
pub const PTE_PPN_MASK: PteVal = ((1u64 << 44) - 1) << 10;
#[cfg(target_pointer_width = "32")]
pub const PTE_PPN_MASK: PteVal = ((1u32 << 20) - 1) << 10;

/// PTE flags mask (bottom 10 bits).
pub const PTE_FLAGS_MASK: PteVal = 0x3FF;

// ── Page table geometry ────────────────────────────────────────────────

/// Number of PTEs per page table.
/// - Sv39: 512 (9 bits per level)
/// - Sv32: 1024 (10 bits per level)
#[cfg(target_pointer_width = "64")]
pub const PTES_PER_TABLE: usize = 512;
#[cfg(target_pointer_width = "32")]
pub const PTES_PER_TABLE: usize = 1024;

/// Number of paging levels.
/// - Sv39: 3 levels (L2=1 GiB, L1=2 MiB, L0=4 KiB)
/// - Sv32: 2 levels (L1=4 MiB, L0=4 KiB)
#[cfg(target_pointer_width = "64")]
pub const PAGING_LEVELS: u32 = 3;
#[cfg(target_pointer_width = "32")]
pub const PAGING_LEVELS: u32 = 2;

/// Page size (4 KiB on both).
pub const PAGE_SIZE: usize = 4096;
pub const PAGE_SHIFT: u32 = 12;

// ── VA index functions ─────────────────────────────────────────────────

/// Level-2 (1 GiB) VA index. Sv39 only — Sv32 has no L2.
#[cfg(target_pointer_width = "64")]
#[inline]
pub const fn l2_idx(va: Va) -> usize {
    ((va >> 30) & 0x1FF) as usize
}

/// Level-1 VA index.
/// - Sv39: 2 MiB entries (bits 21-29)
/// - Sv32: 4 MiB entries (bits 22-31)
#[cfg(target_pointer_width = "64")]
#[inline]
pub const fn l1_idx(va: Va) -> usize {
    ((va >> 21) & 0x1FF) as usize
}
#[cfg(target_pointer_width = "32")]
#[inline]
pub const fn l1_idx(va: Va) -> usize {
    ((va >> 22) & 0x3FF) as usize
}

/// Level-0 (4 KiB) VA index.
/// - Sv39: bits 12-20
/// - Sv32: bits 12-21
#[cfg(target_pointer_width = "64")]
#[inline]
pub const fn l0_idx(va: Va) -> usize {
    ((va >> 12) & 0x1FF) as usize
}
#[cfg(target_pointer_width = "32")]
#[inline]
pub const fn l0_idx(va: Va) -> usize {
    ((va >> 12) & 0x3FF) as usize
}

// ── User VA range ──────────────────────────────────────────────────────

/// USER_BASE: start of user VA. Same on both (0x10000 — leaves the
/// first 64 KiB for "null pointer guard").
pub const USER_BASE: u64 = 0x10000;

/// USER_TOP: end of user VA (exclusive).
/// - Sv39: 0x4000_0000 (1 GiB — kernel uses the upper half)
/// - Sv32: 0x8000_0000 (2 GiB — kernel uses the upper 2 GiB)
#[cfg(target_pointer_width = "64")]
pub const USER_TOP: u64 = 0x4000_0000;
#[cfg(target_pointer_width = "32")]
pub const USER_TOP: u64 = 0x8000_0000;

// ── Kernel base ────────────────────────────────────────────────────────

/// KERNEL_BASE: where the kernel image is loaded.
/// - rv64: 0x8020_0000 (above OpenSBI at 0x8000_0000)
/// - rv32: 0x8020_0000 (same, but only 32-bit addressable)
pub const KERNEL_BASE: u64 = 0x8020_0000;

// ── Heap region ────────────────────────────────────────────────────────

pub const USER_HEAP_BASE: u64 = 0x0100_0000;
pub const USER_HEAP_SIZE: u64 = 64 * 1024 * 1024; // 64 MiB
pub const USER_STACK_TOP: u64 = USER_TOP - 4096;
pub const USER_STACK_PAGES: usize = 64;
pub const USER_HEAP_PAGES: usize = 16;
