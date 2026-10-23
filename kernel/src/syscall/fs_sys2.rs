//! Filesystem syscalls (part 2) — `sys_exec`, `sys_sbrk`, `sys_readdir`,
//! `sys_write_fd`, `sys_create`, `sys_mkdir`.
use crate::arch::trap_frame::TrapFrame;
use crate::fs::vfs;
use crate::proc;
use onyx_core::errno::Errno;

use super::handler::{parse_user_path, user_ptr_ok};

#[inline(never)]
/// # Safety: call only from handler::handle's syscall path (current process set, ACL checked); user pointers validated inside.
pub(super) unsafe fn sys_exec(tf: &mut TrapFrame, path: u64, argv: u64) -> i64 {
    // SAFETY: identical syscall context and pointer contract; exec has no envp.
    unsafe { super::fs_sys3::sys_execve(tf, path, argv, 0) }
}

/// # Safety: call only from handler::handle's syscall path (current process set, ACL checked); no user pointers dereferenced.
pub(super) unsafe fn sys_sbrk(incr: i64) -> i64 {
    // SAFETY: by_pid() takes proc_list_lock for the lookup; heap_brk is only mutated by the owning process's own context.
    unsafe {
        let pid = proc::current_pid();
        // Audit fix (🔴 #8): replace `proc::by_pid(pid).unwrap()` with a
        // graceful error return. Same rationale as the brk path in
        // fs_sys3/mem/brk.rs::sys_brk.
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
}

#[inline(never)]
/// # Safety: call only from handler::handle's syscall path (current process set, ACL checked); user pointers validated inside.
pub(super) unsafe fn sys_readdir(dir: u64, name_out: u64, len: u64) -> i64 {
    // SAFETY: name_out/len passed user_ptr_ok and the per-page writable
    // check_user_range above, so vfs::readdir writes only mapped, writable
    // user pages.
    unsafe {
        let mut dir_buf = [0u8; 256];
        let dir_len = match parse_user_path(dir, &mut dir_buf) {
            Some(l) => l,
            None => return Errno::Inval.as_i64(),
        };
        let dir_path = &dir_buf[..dir_len];
        if !user_ptr_ok(name_out, len)
            || crate::mm::vmm::check_user_range(proc::current().root_pa, name_out, len, true)
                .is_err()
        {
            return Errno::Fault.as_i64();
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
}

#[inline(never)]
/// # Safety: call only from handler::handle's syscall path (current process set, ACL checked); user pointers validated inside.
pub(super) unsafe fn sys_write_fd(token: u64, buf: u64, len: u64) -> i64 {
    // SAFETY: buf/len passed user_ptr_ok and the per-page readable check_user_range above, so vfs::write reads only mapped user pages.
    unsafe {
        if !user_ptr_ok(buf, len)
            || crate::mm::vmm::check_user_range(proc::current().root_pa, buf, len, false).is_err()
        {
            return Errno::Fault.as_i64();
        }
        match vfs::write(token, buf as *const u8, len as u32) {
            Ok(n) => n as i64,
            Err(e) => e.as_i64(),
        }
    }
}

#[inline(never)]
/// # Safety: call only from handler::handle's syscall path (current process set, ACL checked); user pointers validated inside.
pub(super) unsafe fn sys_create(path: u64, mode: u64, _reserved: u64) -> i64 {
    // SAFETY: parse_user_path validates the user path internally and copies
    // it into a kernel stack buffer; only that kernel copy is used after.
    unsafe {
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
}

#[inline(never)]
/// # Safety: call only from handler::handle's syscall path (current process set, ACL checked); user pointers validated inside.
pub(super) unsafe fn sys_mkdir(path: u64) -> i64 {
    // SAFETY: parse_user_path validates the user path internally and copies
    // it into a kernel stack buffer; only that kernel copy is used after.
    unsafe {
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
}
