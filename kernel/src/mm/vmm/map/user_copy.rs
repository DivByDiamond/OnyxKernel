//! User-pointer access helpers: range validation and per-page copies
//! between kernel memory and user virtual addresses.
use onyx_core::errno::{Errno, KResult};

/// Validate that every 4 KiB page of user range `[uaddr, uaddr+len)` is
/// mapped as a user leaf (PTE_U), optionally writable, in the given address
/// space. Syscalls must call this before dereferencing user buffers so a
/// bad pointer returns `EFAULT` instead of halting on an S-mode page fault.
///
/// # Safety
///
/// `root_pa` must satisfy the [`crate::mm::vmm`] translate contract (live,
/// direct-mapped root table).
pub unsafe fn check_user_range(root_pa: u64, uaddr: u64, len: u64, write: bool) -> KResult<()> {
    // SAFETY: only read-only PTE translation is performed on a valid root
    // table per the contract above; no memory is written through the
    // translated addresses here.
    unsafe {
        if len == 0 {
            return Ok(());
        }
        let end = uaddr.checked_add(len).ok_or(Errno::Fault)?;
        let mut va = uaddr & !0xFFF;
        while va < end {
            let mapped = if write {
                crate::mm::vmm::translate_user_write(root_pa, va)
            } else {
                crate::mm::vmm::translate_user(root_pa, va)
            };
            if mapped == 0 {
                return Err(Errno::Fault);
            }
            va += 0x1000;
        }
        Ok(())
    }
}

/// Copy `len` bytes from kernel `src` into user VA `[uaddr, uaddr+len)`,
/// re-translating at each 4 KiB frame boundary. Fails with `Errno::Fault`
/// if any covered page is not a writable user mapping.
///
/// # Safety
///
/// `src` must be valid for reads of `len` bytes; `root_pa` must satisfy the
/// translate contract. The caller should hold the VMM lock (or otherwise
/// guarantee the user mappings are stable for the duration of the copy).
pub unsafe fn copy_to_user(root_pa: u64, uaddr: u64, src: *const u8, len: usize) -> KResult<()> {
    // SAFETY: every page of the destination range was validated as a
    // writable user mapping by `check_user_range` immediately above, so
    // each `copy_nonoverlapping` writes exactly `n <= 4 KiB` bytes to
    // translated physical RAM; `src` validity is the caller's contract.
    unsafe {
        check_user_range(root_pa, uaddr, len as u64, true)?;
        let mut done = 0usize;
        while done < len {
            let va = uaddr + done as u64;
            let pa = crate::mm::vmm::translate_user_write(root_pa, va);
            if pa == 0 {
                return Err(Errno::Fault);
            }
            let n = usize::min(len - done, (0x1000 - (va & 0xFFF)) as usize);
            core::ptr::copy_nonoverlapping(src.add(done), pa as *mut u8, n);
            done += n;
        }
        Ok(())
    }
}

/// Copy `len` bytes from user VA `[uaddr, uaddr+len)` into kernel `dst`,
/// re-translating at each 4 KiB frame boundary. Fails with `Errno::Fault`
/// if any covered page is not a readable user mapping.
///
/// # Safety
///
/// `dst` must be valid for writes of `len` bytes; `root_pa` must satisfy
/// the translate contract. Mappings must be stable for the copy duration.
pub unsafe fn copy_from_user(root_pa: u64, dst: *mut u8, uaddr: u64, len: usize) -> KResult<()> {
    // SAFETY: every page of the source range was validated as a readable
    // user mapping by `check_user_range` immediately above, so each
    // `copy_nonoverlapping` reads exactly `n <= 4 KiB` bytes from
    // translated physical RAM; `dst` validity is the caller's contract.
    unsafe {
        check_user_range(root_pa, uaddr, len as u64, false)?;
        let mut done = 0usize;
        while done < len {
            let va = uaddr + done as u64;
            let pa = crate::mm::vmm::translate_user(root_pa, va);
            if pa == 0 {
                return Err(Errno::Fault);
            }
            let n = usize::min(len - done, (0x1000 - (va & 0xFFF)) as usize);
            core::ptr::copy_nonoverlapping(pa as *const u8, dst.add(done), n);
            done += n;
        }
        Ok(())
    }
}
