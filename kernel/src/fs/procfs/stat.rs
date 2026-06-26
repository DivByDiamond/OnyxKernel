use onyx_core::errno::{Errno, KResult};

use super::consts::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ProcfsStat {
    pub ino: u32,
    pub size: u32,
    pub mode: u32,
}

pub unsafe fn stat(ino: u32) -> KResult<ProcfsStat> {
    match ino {
        PROCFS_ROOT_INO => Ok(ProcfsStat {
            ino: PROCFS_ROOT_INO,
            size: 0,
            mode: 0o040755,
        }),
        PROCFS_VERSION_INO => Ok(ProcfsStat {
            ino: PROCFS_VERSION_INO,
            size: VERSION_STR.len() as u32,
            mode: 0o100444,
        }),
        PROCFS_CPUINFO_INO => Ok(ProcfsStat {
            ino: PROCFS_CPUINFO_INO,
            size: PROCFS_MAX_SIZE,
            mode: 0o100444,
        }),
        PROCFS_MEMINFO_INO => Ok(ProcfsStat {
            ino: PROCFS_MEMINFO_INO,
            size: PROCFS_MAX_SIZE,
            mode: 0o100444,
        }),
        PROCFS_UPTIME_INO => Ok(ProcfsStat {
            ino: PROCFS_UPTIME_INO,
            size: 32,
            mode: 0o100444,
        }),
        PROCFS_LOAD_INO => Ok(ProcfsStat {
            ino: PROCFS_LOAD_INO,
            size: PROCFS_MAX_SIZE,
            mode: 0o100444,
        }),
        PROCFS_STAT_INO => Ok(ProcfsStat {
            ino: PROCFS_STAT_INO,
            size: PROCFS_MAX_SIZE,
            mode: 0o100444,
        }),
        _ => Err(Errno::NoEnt),
    }
}
