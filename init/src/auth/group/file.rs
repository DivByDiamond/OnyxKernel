#![expect(dead_code)]

use crate::syscalls;

const TMP_SUFFIX: &[u8] = b".tmp";

pub(crate) fn atomic_rewrite(
    path_nul: &[u8; 64],
    path_len: usize,
    data: &[u8],
    mode: u64,
) -> Result<(), i64> {
    if path_len == 0 || path_len + TMP_SUFFIX.len() >= 64 {
        return Err(-1);
    }
    let mut tmp_buf = [0u8; 64];
    tmp_buf[..path_len].copy_from_slice(&path_nul[..path_len]);
    tmp_buf[path_len..path_len + TMP_SUFFIX.len()].copy_from_slice(TMP_SUFFIX);

    let fd = unsafe { syscalls::create(tmp_buf.as_ptr(), mode, 0) };
    if fd < 0 {
        return Err(fd);
    }
    let write_ret = unsafe { syscalls::write_fd(fd as u64, data.as_ptr(), data.len()) };
    let _ = unsafe { syscalls::fsync(fd as u64) };
    unsafe { syscalls::close(fd as u64) };
    if write_ret < 0 {
        let _ = unsafe { syscalls::unlink(tmp_buf.as_ptr()) };
        return Err(write_ret);
    }
    let ren = unsafe { syscalls::rename(tmp_buf.as_ptr(), path_nul.as_ptr()) };
    if ren < 0 {
        let _ = unsafe { syscalls::unlink(tmp_buf.as_ptr()) };
        return Err(ren);
    }
    Ok(())
}
