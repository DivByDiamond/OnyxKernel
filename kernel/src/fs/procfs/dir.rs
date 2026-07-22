use onyx_core::errno::{Errno, KResult};

use super::consts::*;

pub unsafe fn lookup(name: &[u8]) -> KResult<u32> {
    match name {
        b"" | b"/" | b"." => Ok(PROCFS_ROOT_INO),
        b"version" => Ok(PROCFS_VERSION_INO),
        b"cpuinfo" => Ok(PROCFS_CPUINFO_INO),
        b"meminfo" => Ok(PROCFS_MEMINFO_INO),
        b"uptime" => Ok(PROCFS_UPTIME_INO),
        b"load" => Ok(PROCFS_LOAD_INO),
        b"stat" => Ok(PROCFS_STAT_INO),
        b"modules" => Ok(PROCFS_MODULES_INO),
        _ => Err(Errno::NoEnt),
    }
}

pub unsafe fn readdir_entry(idx: u32, name_out: *mut u8, name_len: usize) -> Option<u32> {
    let (name, ino): (&[u8], u32) = match idx {
        0 => (b"." as &[u8], PROCFS_ROOT_INO),
        1 => (b".." as &[u8], PROCFS_ROOT_INO),
        2 => (b"version" as &[u8], PROCFS_VERSION_INO),
        3 => (b"cpuinfo" as &[u8], PROCFS_CPUINFO_INO),
        4 => (b"meminfo" as &[u8], PROCFS_MEMINFO_INO),
        5 => (b"uptime" as &[u8], PROCFS_UPTIME_INO),
        6 => (b"load" as &[u8], PROCFS_LOAD_INO),
        7 => (b"stat" as &[u8], PROCFS_STAT_INO),
        8 => (b"modules" as &[u8], PROCFS_MODULES_INO),
        _ => return None,
    };
    let n = name.len().min(name_len.saturating_sub(1));
    unsafe {
        core::ptr::copy_nonoverlapping(name.as_ptr(), name_out, n);
        *name_out.add(n) = 0;
    }
    Some(ino)
}
