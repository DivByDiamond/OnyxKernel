//! Filesystem syscalls (part 2) — `sys_exec`, `sys_sbrk`, `sys_readdir`,
//! `sys_write_fd`, `sys_create`, `sys_mkdir`.
use crate::arch::trap_frame::TrapFrame;
use crate::fs::vfs;
use crate::mm::heap;
use crate::proc;
use onyx_core::errno::Errno;

use super::handler::{parse_user_path, user_ptr_ok};

pub(super) unsafe fn sys_exec(tf: &mut TrapFrame, path: u64, argv: u64) -> i64 {
    let mut path_buf = [0u8; 256];
    let path_len = match parse_user_path(path, &mut path_buf) {
        Some(l) => l,
        None => return Errno::Inval.as_i64(),
    };
    let path_bytes = &path_buf[..path_len];
    let cur_ring = proc::current_ring();
    let token = match vfs::open(path_bytes, vfs::PERM_READ | vfs::PERM_SEEK) {
        Ok(t) => t,
        Err(e) => return e.as_i64(),
    };
    let mut size = 0u32;
    vfs::stat(token, &mut size).ok();
    let img = match heap::kmalloc(size as usize) {
        Ok(p) => p,
        Err(e) => return e.as_i64(),
    };
    vfs::read(token, img, size).ok();
    vfs::close(token).ok();
    let r = match proc::onx::load(img, size as usize) {
        Ok(r) => r,
        Err(e) => {
            heap::kfree(img);
            return e.as_i64();
        }
    };
    heap::kfree(img);
    if cur_ring == proc::PROC_RING_USER && r.ring == 1 {
        return Errno::Perm.as_i64();
    }
    let (argc, argv_sp) = proc::onx::copy_argv_to_stack(r.root_pa, r.ustack, argv);
    let p = proc::current();
    // Bug (proc SERIOUS #4): close FD_CLOEXEC file descriptors before
    // replacing the process image, matching sys_execve's behavior.
    // Without this, file descriptors marked close-on-exec would leak
    // across exec() (sys_execve handled it, but sys_exec didn't).
    for i in 0..p.fds.len() {
        if p.fds[i].used && p.fds[i].cloexec {
            let token = crate::fs::vfs::fd_token(i, p.fds[i].epoch);
            let _ = crate::fs::vfs::close(token);
        }
    }
    // Bug #8 fix: port the root_refcount logic from sys_execve. The previous
    // code unconditionally destroyed the old root page table, even when
    // fork() had shared it with a child via root_refcount. After fork+exec,
    // the child's root_pa pointer would dangle into freed page-table memory
    // — a classic UAF. Now we decrement the refcount and only destroy when
    // it reaches zero, and we allocate a fresh refcount for the new root.
    if p.root_pa != 0 {
        if !p.root_refcount.is_null() {
            *p.root_refcount -= 1;
            if *p.root_refcount == 0 {
                heap::kfree(p.root_refcount as *mut u8);
                crate::mm::vmm::destroy_root(p.root_pa);
            }
        } else {
            crate::mm::vmm::destroy_root(p.root_pa);
        }
    }
    p.root_pa = r.root_pa;
    // Allocate a fresh refcount for the new root page table.
    match heap::kmalloc(4) {
        Ok(rc) => {
            let rcp = rc as *mut u32;
            *rcp = 1;
            p.root_refcount = rcp;
        }
        Err(e) => return e.as_i64(),
    }
    p.entry = r.entry;
    p.ustack = if argc > 0 { argv_sp } else { r.ustack };
    p.heap_brk = r.heap_brk;
    p.readdir_ino = 0;
    p.readdir_idx = 0;
    p.readdir_active = false;
    p.readdir_fs = crate::fs::vfs::Fs::None;
    p.ring = if r.ring == 1 {
        proc::PROC_RING_ROOT
    } else {
        proc::PROC_RING_USER
    };
    // Set up tf such that after handler does sepc += 4 and a0 = ret,
    // we get correct values:
    //   sepc = entry  (tf.sepc - 4 to compensate for handler's +4)
    //   a0   = argc   (return argc, handler will set a0 = ret)
    tf.sepc = r.entry.wrapping_sub(4);
    tf.sp = if argc > 0 { argv_sp } else { r.ustack };
    tf.a0 = argc as u64;
    tf.a1 = if argc > 0 { argv_sp + 8 } else { 0 };
    tf.sstatus = crate::arch::regs::SSTATUS_SPIE;
    if cfg!(target_pointer_width = "64") {
        tf.satp = crate::arch::regs::SATP_MODE_SV39 | (r.root_pa >> 12);
    } else {
        tf.satp = (crate::arch::bits::SATP_MODE_SV32 as u32 | ((r.root_pa >> 12) & 0x3FFFFF) as u32)
            as crate::arch::trap_frame::Reg;
    }
    argc as i64
}

pub(super) unsafe fn sys_sbrk(incr: i64) -> i64 {
    let pid = proc::current_pid();
    // Audit fix (🔴 #8): replace `proc::by_pid(pid).unwrap()` with a
    // graceful error return. See fs_sys3/mem.rs::sys_sbrk for the full
    // rationale — same fix, different call site (this is the live
    // sys_sbrk, mem.rs::sys_sbrk is the dead_code-tagged twin).
    let p = match proc::by_pid(pid) {
        Some(proc) => proc,
        None => return Errno::Inval.as_i64(),
    };
    let cur = p.heap_brk;
    let heap_end = crate::arch::regs::USER_HEAP_BASE + crate::arch::regs::USER_HEAP_SIZE;
    // Bug (syscall SERIOUS #1): use checked arithmetic for the new brk.
    // The previous code did `(cur as i64 + incr) as u64` which silently
    // wraps on signed overflow — e.g. sbrk(i64::MIN) or sbrk(very large)
    // would produce a tiny new_brk that passes the bounds check and
    // corrupt the heap.
    let new_brk = match (cur as i64).checked_add(incr) {
        Some(v) => v as u64,
        None => return Errno::NoMem.as_i64(),
    };
    if new_brk < crate::arch::regs::USER_HEAP_BASE || new_brk > heap_end {
        return Errno::NoMem.as_i64();
    }
    p.heap_brk = new_brk;
    cur as i64
}

pub(super) unsafe fn sys_readdir(dir: u64, name_out: u64, len: u64) -> i64 {
    let mut dir_buf = [0u8; 256];
    let dir_len = match parse_user_path(dir, &mut dir_buf) {
        Some(l) => l,
        None => return Errno::Inval.as_i64(),
    };
    let dir_path = &dir_buf[..dir_len];
    if !user_ptr_ok(name_out, len) {
        return Errno::Inval.as_i64();
    }
    match vfs::readdir(dir_path, name_out as *mut u8, len as usize) {
        Ok(has_entry) => {
            if has_entry {
                1
            } else {
                0
            }
        }
        Err(e) => e.as_i64(),
    }
}

pub(super) unsafe fn sys_write_fd(token: u64, buf: u64, len: u64) -> i64 {
    if !user_ptr_ok(buf, len) {
        return Errno::Inval.as_i64();
    }
    match vfs::write(token, buf as *const u8, len as u32) {
        Ok(n) => n as i64,
        Err(e) => e.as_i64(),
    }
}

pub(super) unsafe fn sys_create(path: u64, mode: u64, _reserved: u64) -> i64 {
    let mut path_buf = [0u8; 256];
    let path_len = match parse_user_path(path, &mut path_buf) {
        Some(l) => l,
        None => return Errno::Inval.as_i64(),
    };
    let path_bytes = &path_buf[..path_len];
    let mode_u32 = if mode == 0 {
        onyx_core::formats::ONYFS_DT_REG
    } else {
        mode as u32
    };
    match vfs::create(path_bytes, mode_u32) {
        Ok(token) => token as i64,
        Err(e) => e.as_i64(),
    }
}

pub(super) unsafe fn sys_mkdir(path: u64) -> i64 {
    let mut path_buf = [0u8; 256];
    let path_len = match parse_user_path(path, &mut path_buf) {
        Some(l) => l,
        None => return Errno::Inval.as_i64(),
    };
    let path_bytes = &path_buf[..path_len];
    match vfs::mkdir(path_bytes) {
        Ok(()) => 0,
        Err(e) => e.as_i64(),
    }
}
