use onyx_core::errno::Errno;

use crate::fs::vfs;
use crate::mm::vmm;
use crate::proc;
use crate::syscall::handler::user_ptr_ok;

pub unsafe fn sys_getdents64(fd: u64, buf: u64, count: u64) -> i64 {
    unsafe {
        if !user_ptr_ok(buf, count) || count < 19 {
            return Errno::Inval.as_i64();
        }
        let root_pa = proc::current().root_pa;
        // Every page the user buffer spans must be mapped PTE_U|W before we
        // start filling, otherwise a partial fill could hit an S-mode fault.
        if vmm::check_user_range(root_pa, buf, count, true).is_err() {
            return Errno::Fault.as_i64();
        }

        let idx = match vfs::fd_check(fd) {
            Ok(i) => i,
            Err(e) => return e.as_i64(),
        };
        let f = vfs::fd_get(idx);

        let mut cursor = f.pos;
        let mut written = 0u64;

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
                    // Build the full dirent record in kernel memory, then copy
                    // it out with per-page translation so a record straddling
                    // two physical frames is written correctly instead of
                    // running past the end of a single translated page.
                    let mut rec = [0u8; 288];
                    let p = rec.as_mut_ptr();
                    *(p as *mut u64) = d_ino as u64;
                    *(p.add(8) as *mut u64) = 0;
                    *(p.add(16) as *mut u16) = reclen_aligned;
                    p.add(18).write(0);
                    core::ptr::copy_nonoverlapping(entry_buf.as_ptr(), p.add(19), name_len);
                    // Padding after the name stays zero from the initializer,
                    // matching the previous write_bytes behaviour.

                    if vmm::copy_to_user(
                        root_pa,
                        buf + written,
                        rec.as_ptr(),
                        reclen_aligned as usize,
                    )
                    .is_err()
                    {
                        return Errno::Fault.as_i64();
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
}

pub unsafe fn sys_getdents(fd: u64, buf: u64, count: u64) -> i64 {
    unsafe { sys_getdents64(fd, buf, count) }
}

pub unsafe fn sys_getentropy(buf: u64, len: u64) -> i64 {
    unsafe {
        if len > 256 || !user_ptr_ok(buf, len) {
            return Errno::Inval.as_i64();
        }
        let root_pa = proc::current().root_pa;
        if vmm::check_user_range(root_pa, buf, len, true).is_err() {
            return Errno::Fault.as_i64();
        }
        // Fill in small chunks from the hardware RNG and copy each chunk out
        // with per-page translation, so a buffer straddling a page boundary
        // is handled correctly.
        let mut chunk = [0u8; 64];
        let mut done = 0usize;
        while done < len as usize {
            let n = core::cmp::min(chunk.len(), len as usize - done);
            crate::drivers::hwrand::fill(&mut chunk[..n]);
            if vmm::copy_to_user(root_pa, buf + done as u64, chunk.as_ptr(), n).is_err() {
                return Errno::Fault.as_i64();
            }
            done += n;
        }
        0
    }
}
