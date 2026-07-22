use core::ptr;
use onyx_core::errno::Errno;

use crate::fs::vfs;
use crate::mm::vmm;
use crate::proc;
use crate::syscall::handler::user_ptr_ok;

pub unsafe fn sys_getdents64(fd: u64, buf: u64, count: u64) -> i64 {
    if !user_ptr_ok(buf, count) || count < 19 {
        return Errno::Inval.as_i64();
    }

    let idx = match vfs::fd_check(fd) {
        Ok(i) => i,
        Err(e) => return e.as_i64(),
    };
    let f = vfs::fd_get(idx);

    let pa = match crate::mm::vmm::translate(proc::current().root_pa, buf) {
        0 => return Errno::Inval.as_i64(),
        p => p,
    };

    let mut cursor = f.pos;
    let mut written = 0u64;
    let dst = pa as *mut u8;

    loop {
        let mut entry_buf = [0u8; 256];
        match vfs::readdir_entry_by_ino(f.fs, f.ino, cursor, entry_buf.as_mut_ptr(), 256) {
            Ok(Some(d_ino)) => {
                let name_len = entry_buf.iter().position(|&b| b == 0).unwrap_or(0);
                let reclen = 19 + name_len as u16;
                let reclen_aligned = (reclen + 7) & !7;
                if written + reclen_aligned as u64 > count {
                    break;
                }
                let p = dst.add(written as usize);
                *(p as *mut u64) = d_ino as u64;
                *(p.add(8) as *mut u64) = 0;
                *(p.add(16) as *mut u16) = reclen_aligned;
                p.add(18).write(0);
                core::ptr::copy_nonoverlapping(entry_buf.as_ptr(), p.add(19), name_len);
                if reclen_aligned > reclen {
                    core::ptr::write_bytes(
                        p.add(19 + name_len as usize),
                        0,
                        (reclen_aligned - reclen) as usize,
                    );
                }
                written += reclen_aligned as u64;
                cursor += 1;
            }
            Ok(None) => break,
            Err(e) => return e.as_i64(),
        }
    }

    vfs::fd_update_pos(idx, cursor);
    written as i64
}

pub unsafe fn sys_getdents(fd: u64, buf: u64, count: u64) -> i64 {
    sys_getdents64(fd, buf, count)
}

pub unsafe fn sys_getentropy(buf: u64, len: u64) -> i64 {
    if len > 256 || !user_ptr_ok(buf, len) {
        return Errno::Inval.as_i64();
    }
    let pa = crate::mm::vmm::translate(proc::current().root_pa, buf);
    if pa == 0 {
        return Errno::Inval.as_i64();
    }
    let dst = pa as *mut u8;
    let mut seed = crate::srv::timer::uptime_us()
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(proc::current_pid() as u64);
    for i in 0..len {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        *dst.add(i as usize) = seed as u8;
    }
    0
}
