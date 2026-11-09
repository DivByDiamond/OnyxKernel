//! FAT32 path resolution: root → (first cluster, size), plus the containing
//! directory cluster that the write paths need in order to patch dirents.
use onyx_core::errno::{Errno, KResult};

use super::super::super::{G_ROOT_CLUSTER, scan_dir_entries};

/// One resolved path, keeping everything the callers may need.
pub(crate) struct Resolution {
    /// First data cluster of the target (0 for an empty foreign file).
    pub cluster: u32,
    /// File size in bytes from the dirent (0 for directories).
    pub size: u32,
    /// Cluster of the directory that holds the target's dirent.
    pub parent: u32,
    /// True when the target is a directory.
    pub is_dir: bool,
}

/// Root directory cluster of the mounted volume.
///
/// Set once by `mount()` before any VFS user exists, then immutable, so
/// reading it cannot observe a torn value.
pub fn root_cluster() -> u32 {
    // SAFETY: written only by mount() during single-hart boot init and never
    // changed afterwards; a plain u32 read is atomic on all supported harts.
    unsafe { G_ROOT_CLUSTER }
}

/// Resolve one path component inside the directory at `dir_cluster`.
///
/// # Safety
///
/// Caller must not invoke FAT32 I/O concurrently from multiple harts;
/// `dir_cluster` must be a valid data cluster. `component` is a kernel-owned
/// slice matched against both the 8.3 alias and the LFN name.
unsafe fn lookup_component(
    dir_cluster: u32,
    component: &[u8],
    out_cluster: &mut u32,
    out_size: &mut u32,
    out_is_dir: &mut bool,
) -> KResult<()> {
    // SAFETY: delegates to scan_dir_entries, whose contract covers all raw
    // access; the scratch buffer is a valid 512-byte stack allocation.
    unsafe {
        let mut buf = [0u8; 512];
        scan_dir_entries(
            dir_cluster,
            component,
            out_cluster,
            out_size,
            out_is_dir,
            &mut buf,
        )
    }
}

/// Walk `path` from the FAT32 root and return the full resolution.
///
/// # Safety
///
/// Caller must not invoke FAT32 I/O concurrently from multiple harts.
/// `path` must be a kernel-owned byte string - the syscall layer parses
/// user paths (parse_user_path) before reaching fs/.
unsafe fn walk(path: &[u8]) -> KResult<Resolution> {
    // SAFETY: single-threaded FAT32 exclusion (see # Safety); all raw access
    // is delegated to lookup_component (bounds-checked).
    unsafe {
        let root = root_cluster();
        if path.is_empty() || path == b"/" || path == b"." {
            return Ok(Resolution {
                cluster: root,
                size: 0,
                parent: root,
                is_dir: true,
            });
        }
        let mut current_cluster = root;
        let mut pos = 0usize;
        let len = path.len();
        loop {
            while pos < len && path[pos] == b'/' {
                pos += 1;
            }
            if pos >= len {
                // Trailing slashes only: the path names `current_cluster`.
                return Ok(Resolution {
                    cluster: current_cluster,
                    size: 0,
                    parent: current_cluster,
                    is_dir: true,
                });
            }
            let start = pos;
            while pos < len && path[pos] != b'/' {
                pos += 1;
            }
            let component = &path[start..pos];
            let mut found_cluster = 0u32;
            let mut found_size = 0u32;
            let mut is_dir = false;
            lookup_component(
                current_cluster,
                component,
                &mut found_cluster,
                &mut found_size,
                &mut is_dir,
            )?;
            if pos >= len {
                return Ok(Resolution {
                    cluster: found_cluster,
                    size: found_size,
                    parent: current_cluster,
                    is_dir,
                });
            }
            if !is_dir {
                return Err(Errno::NotDir);
            }
            current_cluster = found_cluster;
        }
    }
}

/// Resolve an absolute path from the FAT32 root to (cluster, size).
///
/// # Safety
///
/// Caller must not invoke FAT32 I/O concurrently from multiple harts.
/// `path` must be a kernel-owned byte string (see `walk`).
pub unsafe fn lookup(path: &[u8], out_cluster: &mut u32, out_size: &mut u32) -> KResult<()> {
    // SAFETY: walk carries the FAT32 exclusion + kernel-path contract.
    unsafe {
        let r = walk(path)?;
        *out_cluster = r.cluster;
        *out_size = r.size;
        Ok(())
    }
}

/// Resolve a path and also report the cluster of its parent directory, which
/// the write paths store in the fd so a growing write can patch the dirent's
/// size field without walking the path again.
///
/// # Safety
///
/// Same contract as `lookup`.
pub unsafe fn lookup_full(
    path: &[u8],
    out_cluster: &mut u32,
    out_size: &mut u32,
    out_parent: &mut u32,
) -> KResult<()> {
    // SAFETY: walk carries the FAT32 exclusion + kernel-path contract.
    unsafe {
        let r = walk(path)?;
        *out_cluster = r.cluster;
        *out_size = r.size;
        *out_parent = r.parent;
        Ok(())
    }
}

/// Resolve a path that must name a directory, returning its cluster.
///
/// # Safety
///
/// Same contract as `lookup`.
pub unsafe fn lookup_dir(path: &[u8]) -> KResult<u32> {
    // SAFETY: walk carries the FAT32 exclusion + kernel-path contract.
    unsafe {
        let r = walk(path)?;
        if !r.is_dir {
            return Err(Errno::NotDir);
        }
        Ok(r.cluster)
    }
}
