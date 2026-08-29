const MAX_ARGV: usize = 64;
const MAX_ARGV_BYTES: usize = 8192;
const MAX_ENVP: usize = 32;
const MAX_ENVP_BYTES: usize = 8192;

mod at {
    pub const AT_NULL: u64 = 0;
    pub const AT_PAGESZ: u64 = 6;
    pub const AT_ENTRY: u64 = 9;
    pub const AT_UID: u64 = 11;
    pub const AT_EUID: u64 = 12;
    pub const AT_GID: u64 = 13;
    pub const AT_EGID: u64 = 14;
    pub const AT_CLKTCK: u64 = 17;
    pub const AT_RANDOM: u64 = 25;
}

mod layout;

pub(crate) use layout::copy_argv_envp_to_stack;

/// Bounds-check a user pointer of `len` bytes using the same range check as
/// the syscall layer (`user_ptr_ok`): [USER_BASE, USER_TOP) with overflow
/// guard, so both the char** array base and each string pointer are validated
/// before any dereference.
///
/// # Safety
///
/// Caller contract: purely a range check (no dereference happens here);
/// `p` may be any value (returns false for out-of-range).
unsafe fn argv_ptr_ok(p: u64, len: u64) -> bool {
    crate::syscall::handler::user_ptr_ok(p, len)
}

/// # Safety
///
/// Caller contract: `root_pa` is a valid root table; `va` must already be
/// mapped as a user page (unmapped = silently skipped).
unsafe fn write_val(root_pa: u64, va: u64, val: u64) {
    // SAFETY: vmm::translate returns a valid PA (or 0, which is skipped)
    // for the mapped user pages of the loader-owned root table.
    unsafe {
        let pa = crate::mm::vmm::translate(root_pa, va);
        if pa != 0 {
            *(pa as *mut u64) = val;
        }
    }
}

/// # Safety
///
/// Caller contract: `p` was validated by argv_ptr_ok (in-range user ptr);
/// `buf` has room per the caller's accounting; reads at most max_len bytes
/// plus a from_raw_parts copy of the measured slen.
unsafe fn copy_user_str(p: u64, buf: &mut [u8], off: &mut usize, max_len: usize) -> Option<usize> {
    // SAFETY: p passed argv_ptr_ok at the call site ([USER_BASE, USER_TOP)
    // with overflow guard) and the scan stops at max_len or NUL, so the raw
    // reads stay inside the validated user range.
    unsafe {
        let mut slen = 0usize;
        while slen < max_len && *((p + slen as u64) as *const u8) != 0 {
            slen += 1;
        }
        if *off + slen + 1 > buf.len() {
            return None;
        }
        buf[*off..*off + slen].copy_from_slice(core::slice::from_raw_parts(p as *const u8, slen));
        *off += slen;
        buf[*off] = 0;
        *off += 1;
        Some(slen)
    }
}

/// # Safety
///
/// Caller contract: `ptrs` (the argv/envp array base) was validated with
/// argv_ptr_ok by the caller; `buf`/`offsets` are caller-owned and sized;
/// each string pointer is re-validated before its bytes are copied.
unsafe fn collect_strings(
    ptrs: *const u64,
    max_count: usize,
    max_str_len: usize,
    buf: &mut [u8],
    offsets: &mut [u64; 64],
    off: &mut usize,
) -> usize {
    // SAFETY: ptrs base was argv_ptr_ok-checked by the caller and reads are
    // capped at max_count; each element p is re-validated with argv_ptr_ok
    // before copy_user_str dereferences it.
    unsafe {
        let mut count = 0usize;
        for i in 0..max_count {
            let p = *ptrs.add(i);
            if p == 0 {
                break;
            }
            if !argv_ptr_ok(p, max_str_len as u64 + 1) {
                break;
            }
            let start = *off;
            if copy_user_str(p, buf, off, max_str_len).is_none() {
                break;
            }
            if count < offsets.len() {
                offsets[count] = start as u64;
            }
            count += 1;
        }
        count
    }
}

/// # Safety
///
/// Caller contract: `root_pa` is a valid root table with the user stack
/// already mapped; `argv_user` (if non-zero) must pass argv_ptr_ok.
pub(crate) unsafe fn copy_argv_to_stack(
    root_pa: u64,
    ustack_top: u64,
    argv_user: u64,
) -> (usize, u64) {
    // SAFETY: forwards to copy_argv_envp_to_stack, whose caller contract
    // (validated user pointers, mapped stack pages) is satisfied here.
    unsafe { copy_argv_envp_to_stack(root_pa, ustack_top, argv_user, 0, 0, 0) }
}
