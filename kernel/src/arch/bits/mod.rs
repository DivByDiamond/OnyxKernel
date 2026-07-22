#![allow(non_upper_case_globals)]

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

mod layout;
mod pte;

#[cfg(target_pointer_width = "64")]
pub type usize_val = u64;
#[cfg(target_pointer_width = "32")]
pub type usize_val = u32;

#[cfg(target_pointer_width = "64")]
pub type PteVal = u64;
#[cfg(target_pointer_width = "32")]
pub type PteVal = u32;

pub type Va = usize_val;
pub type Pa = usize_val;

pub use pte::SATP_MODE_PAGING;
pub use pte::SATP_MODE_SV32;
#[cfg(target_pointer_width = "64")]
pub use pte::SATP_MODE_SV39;

pub use pte::{
    PAGE_SHIFT, PAGE_SIZE, PAGING_LEVELS, PTES_PER_TABLE, PTE_A, PTE_D, PTE_FLAGS_MASK, PTE_G,
    PTE_LEAF, PTE_PPN_MASK, PTE_PPN_SHIFT, PTE_R, PTE_U, PTE_V, PTE_W, PTE_X,
};

#[cfg(target_pointer_width = "64")]
pub use pte::l2_idx;
pub use pte::{l0_idx, l1_idx};

pub use layout::{
    KERNEL_BASE, USER_BASE, USER_HEAP_BASE, USER_HEAP_PAGES, USER_HEAP_SIZE, USER_STACK_PAGES,
    USER_STACK_TOP, USER_TOP,
};
