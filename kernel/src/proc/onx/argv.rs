use crate::arch::regs::*;

const MAX_ARGV: usize = 16;
const MAX_ARGV_BYTES: usize = 1024;

unsafe fn argv_ptr_ok(p: u64) -> bool {
    p >= USER_BASE && p < USER_TOP
}

unsafe fn write_val(root_pa: u64, va: u64, val: u64) {
    let pa = crate::mm::vmm::translate(root_pa, va);
    if pa != 0 { *(pa as *mut u64) = val; }
}

pub(crate) unsafe fn copy_argv_to_stack(root_pa: u64, ustack_top: u64, argv_user: u64) -> (usize, u64) {
    use crate::mm::vmm;
    if argv_user == 0 || !argv_ptr_ok(argv_user) {
        return (0, ustack_top - 16);
    }
    let mut argc = 0usize;
    let mut buf = [0u8; MAX_ARGV_BYTES];
    let mut off = 0usize;
    let ptrs = argv_user as *const u64;
    for i in 0..MAX_ARGV {
        let p = *ptrs.add(i);
        if p == 0 { break; }
        if !argv_ptr_ok(p) { break; }
        let mut slen = 0usize;
        while slen < 127 && *((p + slen as u64) as *const u8) != 0 { slen += 1; }
        if off + slen + 1 > MAX_ARGV_BYTES { break; }
        buf[off..off + slen].copy_from_slice(core::slice::from_raw_parts(p as *const u8, slen));
        off += slen;
        buf[off] = 0;
        off += 1;
        argc += 1;
    }
    if argc == 0 { return (0, ustack_top - 16); }
    let str_size = off;
    let ptr_size = (argc + 1) * 8;
    let total = 8 + ptr_size + str_size;
    let sp = (ustack_top - total as u64) & !15;

    let mut va = sp;
    write_val(root_pa, va, argc as u64); va += 8;
    let str_base = sp + 8 + ptr_size as u64;
    let mut di = 0usize;
    for _ in 0..argc {
        write_val(root_pa, va, str_base + di as u64); va += 8;
        while di < off && buf[di] != 0 { di += 1; }
        di += 1;
    }
    write_val(root_pa, va, 0);

    let dst_pa = vmm::translate(root_pa, sp + 8 + ptr_size as u64);
    if dst_pa != 0 {
        core::ptr::copy_nonoverlapping(buf.as_ptr(), dst_pa as *mut u8, str_size);
    }
    (argc, sp)
}
