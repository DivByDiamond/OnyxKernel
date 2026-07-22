use crate::arch::regs::*;

const MAX_ARGV: usize = 64;
const MAX_ARGV_BYTES: usize = 8192;
const MAX_ENVP: usize = 32;
const MAX_ENVP_BYTES: usize = 8192;

mod at {
    pub const AT_NULL: u64 = 0;
    #[expect(dead_code)]
    pub const AT_IGNORE: u64 = 1;
    #[expect(dead_code)]
    pub const AT_EXECFD: u64 = 2;
    #[expect(dead_code)]
    pub const AT_PHDR: u64 = 3;
    #[expect(dead_code)]
    pub const AT_PHENT: u64 = 4;
    #[expect(dead_code)]
    pub const AT_PHNUM: u64 = 5;
    pub const AT_PAGESZ: u64 = 6;
    #[expect(dead_code)]
    pub const AT_BASE: u64 = 7;
    #[expect(dead_code)]
    pub const AT_FLAGS: u64 = 8;
    pub const AT_ENTRY: u64 = 9;
    pub const AT_UID: u64 = 11;
    pub const AT_EUID: u64 = 12;
    pub const AT_GID: u64 = 13;
    pub const AT_EGID: u64 = 14;
    #[expect(dead_code)]
    pub const AT_PLATFORM: u64 = 15;
    #[expect(dead_code)]
    pub const AT_HWCAP: u64 = 16;
    pub const AT_CLKTCK: u64 = 17;
    pub const AT_RANDOM: u64 = 25;
    #[expect(dead_code)]
    pub const AT_EXECFN: u64 = 31;
}

mod layout;

pub(crate) use layout::copy_argv_envp_to_stack;

unsafe fn argv_ptr_ok(p: u64) -> bool {
    p >= USER_BASE && p < USER_TOP
}

unsafe fn write_val(root_pa: u64, va: u64, val: u64) {
    let pa = crate::mm::vmm::translate(root_pa, va);
    if pa != 0 {
        *(pa as *mut u64) = val;
    }
}

unsafe fn copy_user_str(p: u64, buf: &mut [u8], off: &mut usize, max_len: usize) -> Option<usize> {
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

unsafe fn collect_strings(
    ptrs: *const u64,
    max_count: usize,
    max_str_len: usize,
    buf: &mut [u8],
    offsets: &mut [u64; 64],
    off: &mut usize,
) -> usize {
    let mut count = 0usize;
    for i in 0..max_count {
        let p = *ptrs.add(i);
        if p == 0 {
            break;
        }
        if !argv_ptr_ok(p) {
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

pub(crate) unsafe fn copy_argv_to_stack(
    root_pa: u64,
    ustack_top: u64,
    argv_user: u64,
) -> (usize, u64) {
    copy_argv_envp_to_stack(root_pa, ustack_top, argv_user, 0, 0, 0)
}
