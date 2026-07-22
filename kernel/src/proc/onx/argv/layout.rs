use super::{
    argv_ptr_ok, collect_strings, write_val, MAX_ARGV, MAX_ARGV_BYTES, MAX_ENVP, MAX_ENVP_BYTES,
};
use crate::arch::regs::*;

const AUXV_COUNT: usize = 9;
const AT_RANDOM_BYTES: usize = 16;

pub(crate) unsafe fn copy_argv_envp_to_stack(
    root_pa: u64,
    ustack_top: u64,
    argv_user: u64,
    envp_user: u64,
    entry_vaddr: u64,
    uid: u64,
) -> (usize, u64) {
    use crate::mm::vmm;

    let mut argv_buf = [0u8; MAX_ARGV_BYTES];
    let mut argv_offs = [0u64; 64];
    let mut argv_off = 0usize;
    let argc = if argv_user != 0 && argv_ptr_ok(argv_user) {
        collect_strings(
            argv_user as *const u64,
            MAX_ARGV,
            127,
            &mut argv_buf,
            &mut argv_offs,
            &mut argv_off,
        )
    } else {
        0
    };

    if argc == 0 {
        let _placeholder = b"\x00";
        argv_buf[0] = 0;
        argv_offs[0] = 0;
        argv_off = 1;
    }
    let argc_eff = if argc == 0 { 1 } else { argc };
    let argv_str_size = if argc == 0 { 1 } else { argv_off };

    let mut envp_buf = [0u8; MAX_ENVP_BYTES];
    let mut envp_offs = [0u64; 32];
    let mut envp_off = 0usize;
    let envc = if envp_user != 0 && argv_ptr_ok(envp_user) {
        let mut wide = [0u64; 64];
        let n = collect_strings(
            envp_user as *const u64,
            MAX_ENVP,
            255,
            &mut envp_buf,
            &mut wide,
            &mut envp_off,
        );
        for i in 0..n.min(envp_offs.len()) {
            envp_offs[i] = wide[i];
        }
        n
    } else {
        0
    };
    let envp_str_size = envp_off;

    let mut random_buf = [0u8; AT_RANDOM_BYTES];
    let tick = crate::srv::timer::uptime_us() as u64;
    for i in 0..AT_RANDOM_BYTES {
        random_buf[i] = (tick
            .wrapping_mul(0x9E37_79B9_7F4A_7C15)
            .wrapping_add(i as u64)) as u8;
    }

    let auxv_size = AUXV_COUNT * 16;

    let argv_ptr_size = (argc_eff + 1) * 8;
    let envp_ptr_size = (envc + 1) * 8;
    let total = 8
        + argv_ptr_size
        + envp_ptr_size
        + auxv_size
        + argv_str_size
        + envp_str_size
        + AT_RANDOM_BYTES;
    let sp = (ustack_top - total as u64) & !15;

    let mut va = sp;

    write_val(root_pa, va, argc_eff as u64);
    va += 8;

    let argv_str_base = sp + 8 + argv_ptr_size as u64 + envp_ptr_size as u64 + auxv_size as u64;
    let mut di = 0usize;
    for i in 0..argc_eff {
        let str_off = if i < argc { argv_offs[i] } else { 0 };
        write_val(root_pa, va, argv_str_base + str_off);
        va += 8;
        while di < argv_str_size && argv_buf[di] != 0 {
            di += 1;
        }
        di += 1;
    }
    write_val(root_pa, va, 0);
    va += 8;

    let envp_str_base = argv_str_base + argv_str_size as u64;
    let mut ei = 0usize;
    for i in 0..envc {
        let str_off = if i < envp_offs.len() { envp_offs[i] } else { 0 };
        write_val(root_pa, va, envp_str_base + str_off);
        va += 8;
        while ei < envp_str_size && envp_buf[ei] != 0 {
            ei += 1;
        }
        ei += 1;
    }
    write_val(root_pa, va, 0);
    va += 8;

    let random_va = envp_str_base + envp_str_size as u64;
    let auxv = [
        (super::at::AT_PAGESZ, 4096u64),
        (super::at::AT_CLKTCK, 100u64),
        (super::at::AT_UID, uid),
        (super::at::AT_EUID, uid),
        (super::at::AT_GID, uid),
        (super::at::AT_EGID, uid),
        (super::at::AT_ENTRY, entry_vaddr),
        (super::at::AT_RANDOM, random_va),
        (super::at::AT_NULL, 0u64),
    ];
    for (a_type, a_val) in auxv.iter() {
        write_val(root_pa, va, *a_type);
        va += 8;
        write_val(root_pa, va, *a_val);
        va += 8;
    }

    if argv_str_size > 0 {
        let dst_pa = vmm::translate(root_pa, argv_str_base);
        if dst_pa != 0 {
            core::ptr::copy_nonoverlapping(argv_buf.as_ptr(), dst_pa as *mut u8, argv_str_size);
        }
    } else {
        let dst_pa = vmm::translate(root_pa, argv_str_base);
        if dst_pa != 0 {
            *(dst_pa as *mut u8) = 0;
        }
    }
    if envp_str_size > 0 {
        let dst_pa = vmm::translate(root_pa, envp_str_base);
        if dst_pa != 0 {
            core::ptr::copy_nonoverlapping(envp_buf.as_ptr(), dst_pa as *mut u8, envp_str_size);
        }
    }
    let rdst_pa = vmm::translate(root_pa, random_va);
    if rdst_pa != 0 {
        core::ptr::copy_nonoverlapping(random_buf.as_ptr(), rdst_pa as *mut u8, AT_RANDOM_BYTES);
    }

    (argc_eff, sp)
}
