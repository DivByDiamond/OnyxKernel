use onyx_core::formats::ONYFS_ROOT_INO;

pub const PROCFS_ROOT_INO: u32 = ONYFS_ROOT_INO;
pub const PROCFS_VERSION_INO: u32 = 2;
pub const PROCFS_CPUINFO_INO: u32 = 3;
pub const PROCFS_MEMINFO_INO: u32 = 4;
pub const PROCFS_UPTIME_INO: u32 = 5;
pub const PROCFS_LOAD_INO: u32 = 6;
pub const PROCFS_STAT_INO: u32 = 7;
pub const PROCFS_MAX_INO: u32 = 7;

pub const PROCFS_MAX_SIZE: u32 = 512;
pub const DIRENT_SIZE: usize = 40;

pub(crate) const VERSION_STR: &str = "OnyxKernel v0.3 (Rust) — RISC-V 64 GC\n";
