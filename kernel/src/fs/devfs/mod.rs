//! Devfs — virtual /dev: framebuffer, block and PTY nodes.
//!
//! Path-level operations (`lookup`, `stat`, `read`, `write`, `ioctl`,
//! `mmap`) live in [`vnode`]; this module holds the node namespace and the
//! inode allocation scheme.

use crate::drivers::fb;
use core::ptr;
use onyx_core::errno::{Errno, KResult};

mod blk;
mod pty;
mod vnode;

pub use blk::{blk_ino, is_blk_ino};
pub use pty::pty_ioctl::pty_ioctl;
pub use pty::pty_ioctl::{TIOCGPTN, TIOCGWINSZ, TIOCSWINSZ};
pub use pty::pty_nodes::{
    DEVFS_PTMX_INO, is_pty_ino, pty_poll, ptym_ino, ptym_ino_idx, ptys_ino_idx,
};
pub use vnode::{
    FB_IOCTL_CLAIM_GRAPHICS, FB_IOCTL_GET_INFO, FB_IOCTL_RELEASE_GRAPHICS, ioctl, mmap,
    mmap_prefix, read, stat, write,
};

use pty::pty_nodes;

pub const DEVFS_ROOT_INO: u32 = 1;
pub const DEVFS_FB0_INO: u32 = 2;
pub const DEVFS_HVC0_INO: u32 = 49;
const DEVFS_EXTRA_FB_BASE_INO: u32 = 64;

pub fn fb_ino(index: usize) -> u32 {
    if index == 0 {
        DEVFS_FB0_INO
    } else {
        DEVFS_EXTRA_FB_BASE_INO + index as u32 - 1
    }
}

pub fn is_framebuffer_ino(ino: u32) -> bool {
    framebuffer(ino).is_some()
}

fn framebuffer(ino: u32) -> Option<fb::Framebuffer> {
    let index = if ino == DEVFS_FB0_INO {
        0
    } else {
        ino.checked_sub(DEVFS_EXTRA_FB_BASE_INO)? as usize + 1
    };
    fb::display(index)
}

fn fb_index(name: &[u8]) -> Option<usize> {
    let digits = name.strip_prefix(b"fb")?;
    if digits.is_empty() {
        return None;
    }
    let mut index = 0usize;
    for &c in digits {
        if !c.is_ascii_digit() {
            return None;
        }
        index = index.checked_mul(10)?.checked_add((c - b'0') as usize)?;
    }
    (index < fb::count()).then_some(index)
}

/// Base inode for block devices: /dev/blk<N> has inode DEVFS_BLK_BASE_INO + N.
/// Chosen to avoid collision with root (1), fb0 (2) and legacy blk0 (3).
pub const DEVFS_BLK_BASE_INO: u32 = 16;

pub struct DevfsStat {
    pub ino: u32,
    pub size: u32,
    pub mode: u32,
}

pub fn lookup(name: &[u8]) -> KResult<u32> {
    if name.is_empty() || name == b"." {
        return Ok(DEVFS_ROOT_INO);
    }
    if let Some(ino) = pty_nodes::lookup_name(name) {
        return Ok(ino);
    }
    if name == b"hvc0" && crate::drivers::virtio_console::present() {
        return Ok(DEVFS_HVC0_INO);
    }
    if let Some(index) = fb_index(name) {
        return Ok(fb_ino(index));
    }
    if let Some(idx) = blk::parse_name(name) {
        if idx < crate::drivers::virtio::count() {
            return Ok(blk_ino(idx));
        }
        return Err(Errno::NoEnt);
    }
    Err(Errno::NoEnt)
}

pub fn readdir_entry(idx: u32, name_out: *mut u8, name_len: usize) -> Option<(u32, u8)> {
    if idx < 2 {
        copy_name(if idx == 0 { b"." } else { b".." }, name_out, name_len);
        return Some((DEVFS_ROOT_INO, crate::fs::DT_DIR));
    }
    let index = (idx - 2) as usize;
    if index < fb::count() {
        let mut name = [0u8; 4];
        name[..2].copy_from_slice(b"fb");
        let length = if index < 10 {
            name[2] = b'0' + index as u8;
            3
        } else {
            name[2] = b'0' + (index / 10) as u8;
            name[3] = b'0' + (index % 10) as u8;
            4
        };
        copy_name(&name[..length], name_out, name_len);
        return Some((fb_ino(index), crate::fs::DT_CHR));
    }
    let dev_idx = index - fb::count();
    if dev_idx < crate::drivers::virtio::count() {
        copy_name(blk::name(dev_idx), name_out, name_len);
        return Some((blk_ino(dev_idx), crate::fs::DT_BLK));
    }
    if crate::drivers::virtio_console::present() {
        if dev_idx == crate::drivers::virtio::count() {
            copy_name(b"hvc0", name_out, name_len);
            return Some((DEVFS_HVC0_INO, crate::fs::DT_CHR));
        }
        pty_nodes::readdir_entry(idx - 1, name_out, name_len)
    } else {
        pty_nodes::readdir_entry(idx, name_out, name_len)
    }
}

fn copy_name(name: &[u8], out: *mut u8, max_len: usize) {
    let n = name.len().min(max_len);
    // SAFETY: n <= max_len by construction, so the copy stays within the
    // caller-provided name_out buffer of max_len bytes (no NUL is written
    // by this helper).
    unsafe {
        ptr::copy_nonoverlapping(name.as_ptr(), out, n);
    }
}
