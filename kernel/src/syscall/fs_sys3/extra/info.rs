use core::ptr;
use onyx_core::errno::Errno;

use crate::fs::vfs;
use crate::proc;
use crate::syscall::handler::user_ptr_ok;

pub unsafe fn sys_getdents64(fd: u64, buf: u64, count: u64) -> i64 { unsafe {
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
                        p.add(19 + name_len),
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
}}

pub unsafe fn sys_getdents(fd: u64, buf: u64, count: u64) -> i64 { unsafe {
    sys_getdents64(fd, buf, count)
}}

pub unsafe fn sys_getentropy(buf: u64, len: u64) -> i64 { unsafe {
    if len > 256 || !user_ptr_ok(buf, len) {
        return Errno::Inval.as_i64();
    }
    let pa = crate::mm::vmm::translate(proc::current().root_pa, buf);
    if pa == 0 {
        return Errno::Inval.as_i64();
    }
    let dst = pa as *mut u8;
    // Fill in small chunks from the hardware RNG and copy each chunk
    // straight into the translated user page, so we never hold more than
    // one staging buffer of entropy on the stack.
    let mut chunk = [0u8; 64];
    let mut done = 0usize;
    while done < len as usize {
        let n = core::cmp::min(chunk.len(), len as usize - done);
        crate::drivers::hwrand::fill(&mut chunk[..n]);
        // SAFETY: `dst` is the translated physical address of a mapped,
        // user-accessible buffer (checked by translate + user_ptr_ok) and
        // [done, done+n) stays within `len <= 256`.
        ptr::copy_nonoverlapping(chunk.as_ptr(), dst.add(done), n);
        done += n;
    }
    0
}}
