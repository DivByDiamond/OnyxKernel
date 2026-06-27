//! Build the initial user stack frame for a new process.
//!
//! Layout written to the user stack (lowest address first):
//!
//! ```text
//!   sp ──►  argc            (8 bytes)
//!           argv[0]         (8 bytes)
//!           ...
//!           argv[argc-1]    (8 bytes)
//!           NULL            (8 bytes)   — argv terminator
//!           envp[0]         (8 bytes)
//!           ...
//!           envp[envc-1]    (8 bytes)
//!           NULL            (8 bytes)   — envp terminator
//!           auxv[0].a_type  (8 bytes)
//!           auxv[0].a_val   (8 bytes)
//!           ...
//!           auxv[N-1].a_type=AT_NULL (8 bytes)
//!           auxv[N-1].a_val = 0       (8 bytes)
//!           <string blob for argv[] then envp[]>
//!           <16 bytes of randomness for AT_RANDOM>
//!   ustack_top
//! ```
//!
//! This matches the SysV/ELF ABI used by glibc, musl, picolibc — and by the
//! OnyxCC `shim.c::_start` (which reads `a0=argc`, `a1=argv`). The classic
//! libonyxc `start.c::_start` simply ignores these and calls `main(void)`.
use crate::arch::regs::*;

const MAX_ARGV: usize = 64;
const MAX_ARGV_BYTES: usize = 8192;
const MAX_ENVP: usize = 32;
const MAX_ENVP_BYTES: usize = 8192;

/// Auxiliary vector types we emit. Subset of the POSIX/ELF `<elf.h>` `AT_*`.
mod at {
    pub const AT_NULL: u64 = 0;
    pub const AT_IGNORE: u64 = 1;
    pub const AT_EXECFD: u64 = 2;
    pub const AT_PHDR: u64 = 3;
    pub const AT_PHENT: u64 = 4;
    pub const AT_PHNUM: u64 = 5;
    pub const AT_PAGESZ: u64 = 6;
    pub const AT_BASE: u64 = 7;
    pub const AT_FLAGS: u64 = 8;
    pub const AT_ENTRY: u64 = 9;
    pub const AT_UID: u64 = 11;
    pub const AT_EUID: u64 = 12;
    pub const AT_GID: u64 = 13;
    pub const AT_EGID: u64 = 14;
    pub const AT_PLATFORM: u64 = 15;
    pub const AT_HWCAP: u64 = 16;
    pub const AT_CLKTCK: u64 = 17;
    pub const AT_RANDOM: u64 = 25;
    pub const AT_EXECFN: u64 = 31;
}

unsafe fn argv_ptr_ok(p: u64) -> bool {
    p >= USER_BASE && p < USER_TOP
}

unsafe fn write_val(root_pa: u64, va: u64, val: u64) {
    let pa = crate::mm::vmm::translate(root_pa, va);
    if pa != 0 { *(pa as *mut u64) = val; }
}

/// Copy a NUL-terminated string from user space into `buf` starting at `off`.
/// Returns the length of the string (excluding NUL), or None if it doesn't fit
/// or is invalid.
unsafe fn copy_user_str(p: u64, buf: &mut [u8], off: &mut usize, max_len: usize) -> Option<usize> {
    let mut slen = 0usize;
    while slen < max_len && *((p + slen as u64) as *const u8) != 0 { slen += 1; }
    if *off + slen + 1 > buf.len() { return None; }
    buf[*off..*off + slen].copy_from_slice(core::slice::from_raw_parts(p as *const u8, slen));
    *off += slen;
    buf[*off] = 0;
    *off += 1;
    Some(slen)
}

/// Walk an argv/envp array (`*const u64` terminated by NULL) and copy each
/// string into `buf`. Returns the count of items and updates `off` to point
/// past the last copied NUL. Records the per-item string offsets in `offsets`
/// so we can later write pointer values into the stack frame.
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
        if p == 0 { break; }
        if !argv_ptr_ok(p) { break; }
        let start = *off;
        if copy_user_str(p, buf, off, max_str_len).is_none() { break; }
        if count < offsets.len() {
            offsets[count] = start as u64;
        }
        count += 1;
    }
    count
}

/// Build the initial user stack frame.
///
/// `argv_user` and `envp_user` are user pointers to NULL-terminated arrays of
/// `char *` (each `char *` is itself a user pointer to a NUL-terminated
/// string). Either may be 0 / NULL — in that case the corresponding section
/// is emitted as a single NULL terminator (and argc / envc will be 0).
///
/// Returns `(argc, sp)`. The trap-return code sets `a0 = argc` and
/// `a1 = sp + 8` (pointer to `argv[0]`).
pub(crate) unsafe fn copy_argv_to_stack(root_pa: u64, ustack_top: u64, argv_user: u64) -> (usize, u64) {
    copy_argv_envp_to_stack(root_pa, ustack_top, argv_user, 0, 0, 0)
}

/// Full version that also writes envp and auxv. Used by `execve`.
pub(crate) unsafe fn copy_argv_envp_to_stack(
    root_pa: u64,
    ustack_top: u64,
    argv_user: u64,
    envp_user: u64,
    entry_vaddr: u64,
    uid: u64,
) -> (usize, u64) {
    use crate::mm::vmm;

    // ── Collect argv strings ───────────────────────────────────────────────
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
    } else { 0 };

    // Guarantee argv[0] exists — Linux always provides at least the program
    // name. If the caller passed NULL, synthesize a placeholder so libc
    // implementations that assume `argv[0] != NULL` (musl, glibc) work.
    if argc == 0 {
        let placeholder = b"\x00";
        argv_buf[0] = 0;
        argv_offs[0] = 0;
        argv_off = placeholder.len();
    }
    let argc_eff = if argc == 0 { 1 } else { argc };
    let argv_str_size = if argc == 0 { 1 } else { argv_off };

    // ── Collect envp strings ───────────────────────────────────────────────
    let mut envp_buf = [0u8; MAX_ENVP_BYTES];
    let mut envp_offs = [0u64; 32];
    let mut envp_off = 0usize;
    let envc = if envp_user != 0 && argv_ptr_ok(envp_user) {
        // Reuse collect_strings but with a 32-entry offsets buffer — we cast
        // safely because we only index up to envc ≤ 32.
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
    } else { 0 };
    let envp_str_size = envp_off;

    // ── Build auxv ─────────────────────────────────────────────────────────
    // Reserve space for 16 bytes of AT_RANDOM entropy (placed at end of strings).
    const AT_RANDOM_BYTES: usize = 16;
    let mut random_buf = [0u8; AT_RANDOM_BYTES];
    // Cheap entropy from the kernel timer + a per-call counter. Not crypto-
    // secure, but sufficient for stack-canary / ASLR seeding.
    let tick = crate::srv::timer::uptime_us() as u64;
    for i in 0..AT_RANDOM_BYTES {
        random_buf[i] = (tick.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(i as u64)) as u8;
    }

    // We'll emit these auxv entries (each is 16 bytes — type + val):
    //   AT_PAGESZ=4096, AT_CLKTCK=100, AT_UID, AT_EUID, AT_GID, AT_EGID,
    //   AT_ENTRY, AT_RANDOM, AT_NULL
    // That's 9 entries * 16 bytes = 144 bytes.
    const AUXV_COUNT: usize = 9;
    let auxv_size = AUXV_COUNT * 16;

    // ── Compute total size & layout ────────────────────────────────────────
    let argv_ptr_size = (argc_eff + 1) * 8;
    let envp_ptr_size = (envc + 1) * 8;
    let total = 8 /*argc*/
        + argv_ptr_size
        + envp_ptr_size
        + auxv_size
        + argv_str_size
        + envp_str_size
        + AT_RANDOM_BYTES;
    let sp = (ustack_top - total as u64) & !15;

    let mut va = sp;

    // argc
    write_val(root_pa, va, argc_eff as u64); va += 8;

    // argv[] pointers
    let argv_str_base = sp + 8 + argv_ptr_size as u64 + envp_ptr_size as u64 + auxv_size as u64;
    let mut di = 0usize;
    for i in 0..argc_eff {
        let str_off = if i < argc { argv_offs[i] } else { 0 };
        write_val(root_pa, va, argv_str_base + str_off); va += 8;
        // advance `di` past this string in the blob
        while di < argv_str_size && argv_buf[di] != 0 { di += 1; }
        di += 1;
    }
    write_val(root_pa, va, 0); va += 8; // argv NULL terminator

    // envp[] pointers
    let envp_str_base = argv_str_base + argv_str_size as u64;
    let mut ei = 0usize;
    for i in 0..envc {
        let str_off = if i < envp_offs.len() { envp_offs[i] } else { 0 };
        write_val(root_pa, va, envp_str_base + str_off); va += 8;
        while ei < envp_str_size && envp_buf[ei] != 0 { ei += 1; }
        ei += 1;
    }
    write_val(root_pa, va, 0); va += 8; // envp NULL terminator

    // auxv
    let random_va = envp_str_base + envp_str_size as u64;
    let auxv = [
        (at::AT_PAGESZ, 4096u64),
        (at::AT_CLKTCK, 100u64),
        (at::AT_UID, uid),
        (at::AT_EUID, uid),
        (at::AT_GID, uid),
        (at::AT_EGID, uid),
        (at::AT_ENTRY, entry_vaddr),
        (at::AT_RANDOM, random_va),
        (at::AT_NULL, 0u64),
    ];
    for (a_type, a_val) in auxv.iter() {
        write_val(root_pa, va, *a_type); va += 8;
        write_val(root_pa, va, *a_val);  va += 8;
    }

    // ── Copy the string blobs and randomness into user memory ──────────────
    if argv_str_size > 0 {
        let dst_pa = vmm::translate(root_pa, argv_str_base);
        if dst_pa != 0 {
            core::ptr::copy_nonoverlapping(
                argv_buf.as_ptr(),
                dst_pa as *mut u8,
                argv_str_size,
            );
        }
    } else {
        // argc==0 case: write a single NUL so argv[0] is a valid "".
        let dst_pa = vmm::translate(root_pa, argv_str_base);
        if dst_pa != 0 { *(dst_pa as *mut u8) = 0; }
    }
    if envp_str_size > 0 {
        let dst_pa = vmm::translate(root_pa, envp_str_base);
        if dst_pa != 0 {
            core::ptr::copy_nonoverlapping(
                envp_buf.as_ptr(),
                dst_pa as *mut u8,
                envp_str_size,
            );
        }
    }
    let rdst_pa = vmm::translate(root_pa, random_va);
    if rdst_pa != 0 {
        core::ptr::copy_nonoverlapping(
            random_buf.as_ptr(),
            rdst_pa as *mut u8,
            AT_RANDOM_BYTES,
        );
    }

    (argc_eff, sp)
}
