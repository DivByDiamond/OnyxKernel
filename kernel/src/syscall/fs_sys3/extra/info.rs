use onyx_core::errno::Errno;

use crate::fs::vfs;
use crate::mm::vmm;
use crate::proc;
use crate::syscall::handler::user_ptr_ok;

/// # Safety
///
/// Call only from handler::handle's syscall path: current process set, ACL
/// checked; `buf`/`count` are validated inside before any write.
pub unsafe fn sys_getdents64(fd: u64, buf: u64, count: u64) -> i64 {
    // SAFETY: buf/count passed user_ptr_ok and the per-page writable
    // check_user_range above; dirent records are built in a 288-byte stack
    // buffer written strictly in-bounds (reclen_aligned <= 280), then
    // copy_to_user re-validates each destination page before writing.
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
            match vfs::readdir_entry_by_ino(f.fs, f.ino, cursor, entry_buf.as_mut_ptr(), 256, f.mnt)
            {
                Ok(Some((d_ino, d_type))) => {
                    let name_len = entry_buf.iter().position(|&b| b == 0).unwrap_or(0);
                    // Build the full dirent record in kernel memory, then copy
                    // it out with per-page translation so a record straddling
                    // two physical frames is written correctly instead of
                    // running past the end of a single translated page.
                    let mut rec = [0u8; 288];
                    let reclen_aligned = build_dirent(
                        &mut rec,
                        d_ino,
                        u64::from(cursor) + 1,
                        d_type,
                        &entry_buf[..name_len],
                    );
                    if written + reclen_aligned as u64 > count {
                        // Linux returns EINVAL when even the first record does
                        // not fit; 0 would be indistinguishable from EOF.
                        if written == 0 {
                            return Errno::Inval.as_i64();
                        }
                        break;
                    }

                    if vmm::copy_to_user(
                        root_pa,
                        buf + written,
                        rec.as_ptr(),
                        reclen_aligned as usize,
                    )
                    .is_err()
                    {
                        // Records copied so far stay copied: persist the
                        // cursor first, otherwise the next call would
                        // re-emit them and skip nothing.
                        vfs::fd_update_pos(idx, cursor);
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

/// Build one `linux_dirent64` record: `ino`(8) `off`(8) `reclen`(2)
/// `type`(1) `name`(NUL-terminated, padded to 8). `d_off` is the cookie of
/// the *next* entry, so `lseek(fd, d_off, SEEK_SET)` resumes iteration.
/// Returns the 8-byte-aligned record length (always `>= 24`, so at least
/// one entry fits any buffer that passes the `count >= 19` gate after
/// rounding).
fn build_dirent(rec: &mut [u8; 288], d_ino: u32, d_off: u64, d_type: u8, name: &[u8]) -> usize {
    // +1 for the NUL: without it a name ending exactly on an 8-byte
    // boundary would leave the record with no terminator byte.
    let reclen = ((19 + name.len() + 1) as u16 + 7) & !7;
    rec[0..4].copy_from_slice(&d_ino.to_le_bytes());
    rec[4..8].copy_from_slice(&[0; 4]); // high half of d_ino stays zero
    rec[8..16].copy_from_slice(&d_off.to_le_bytes());
    rec[16..18].copy_from_slice(&reclen.to_le_bytes());
    rec[18] = d_type;
    // Padding after the name stays zero from the caller's zeroed buffer.
    rec[19..19 + name.len()].copy_from_slice(name);
    rec[19 + name.len()] = 0;
    reclen as usize
}

/// # Safety
///
/// Call only from handler::handle's syscall path; arguments are forwarded
/// unchanged to sys_getdents64, which validates them internally.
pub unsafe fn sys_getdents(fd: u64, buf: u64, count: u64) -> i64 {
    // SAFETY: the only unsafe operation is the call to sys_getdents64,
    // which validates the user buffer (range + per-page mapping) before
    // writing.
    unsafe { sys_getdents64(fd, buf, count) }
}

#[cfg(test)]
mod tests {
    use super::build_dirent;

    #[test]
    fn dirent_record_layout_matches_linux_dirent64() {
        let mut rec = [0u8; 288];
        let n = build_dirent(&mut rec, 0x1234, 7, 4, b"sub");
        // 19 header bytes + name + NUL, rounded up to 8.
        assert_eq!(n, 24);
        assert_eq!(&rec[0..8], &[0x34, 0x12, 0, 0, 0, 0, 0, 0]);
        // d_off is the cookie of the next entry, not zero.
        assert_eq!(&rec[8..16], &7u64.to_le_bytes());
        assert_eq!(&rec[16..18], &24u16.to_le_bytes());
        assert_eq!(rec[18], 4); // d_type = DT_DIR
        assert_eq!(&rec[19..22], b"sub");
        assert_eq!(rec[22], 0); // name is NUL-terminated inside the record
        assert_eq!(n % 8, 0);
    }

    #[test]
    fn a_name_on_an_8_byte_boundary_still_gets_a_nul() {
        // 19 + 8 + 1 = 28 -> rounded to 32; the NUL must land at index 27,
        // inside the record, instead of bleeding into the next one.
        let mut rec = [0u8; 288];
        let n = build_dirent(&mut rec, 1, 1, 8, b"12345678");
        assert_eq!(n, 32);
        assert_eq!(rec[19 + 8], 0);
        assert!(rec[28..32].iter().all(|&b| b == 0));
    }

    #[test]
    fn a_long_name_stays_within_the_record_buffer() {
        let mut rec = [0u8; 288];
        let name = [b'a'; 255];
        let n = build_dirent(&mut rec, 9, 3, 8, &name);
        assert_eq!(n, 280); // 19 + 255 + 1 = 275 -> 280
        assert_eq!(rec[19 + 255], 0);
        assert!(n <= rec.len());
    }
}

/// # Safety
///
/// Call only from handler::handle's syscall path: current process set, ACL
/// checked; `buf`/`len` are validated inside before any write.
pub unsafe fn sys_getentropy(buf: u64, len: u64) -> i64 {
    // SAFETY: buf/len passed user_ptr_ok and the per-page writable
    // check_user_range above; each chunk is filled in kernel memory by
    // hwrand::fill and copy_to_user re-validates each destination page
    // before writing.
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
