//! Early kernel command line: `/chosen/bootargs` parsed once at boot into a
//! fixed static store (no heap, usable before the allocator is up).
//!
//! Grammar: whitespace-separated tokens of the form `key` or `key=value`.
//! Recognised keys:
//!   - `loglevel=info|warn|error` (default: info)
//!   - `console=uart|fb|none`     (stored; only log filtering is wired now)
//!
//! Any other token stays reachable through [`get`] (e.g. `root=`, `fb=`).
//! A missing `/chosen/bootargs` (QEMU smode fw_jump) is not an error: the
//! defaults simply stay in effect.

use crate::srv::klog::{self, Level};
use core::sync::atomic::{AtomicU8, Ordering};

const BUF_LEN: usize = 256;
const MAX_ENTRIES: usize = 16;

/// Half-open byte range into [`BUF`].
#[derive(Clone, Copy)]
struct Range {
    start: usize,
    end: usize,
}

impl Range {
    const EMPTY: Self = Self { start: 0, end: 0 };
}

#[derive(Clone, Copy)]
struct Entry {
    key: Range,
    val: Range,
}

static mut BUF: [u8; BUF_LEN] = [0; BUF_LEN];
static mut ENTRIES: [Entry; MAX_ENTRIES] = [Entry {
    key: Range::EMPTY,
    val: Range::EMPTY,
}; MAX_ENTRIES];
static mut BUF_USED: usize = 0;
static mut N_ENTRIES: usize = 0;
static PARSED: AtomicU8 = AtomicU8::new(0);

/// Walks the FDT for `/chosen/bootargs` and tokenises it into the static
/// store. Call once, right after `libfdt::fdt::init()` and before any
/// consumer; later calls are ignored.
pub fn init() {
    if PARSED.swap(1, Ordering::Relaxed) != 0 {
        return;
    }
    let mut len = 0usize;
    let mut src = [0u8; BUF_LEN];
    unsafe {
        crate::libfdt::fdt::walk(&mut |name, props: &[(u32, &[u8])]| {
            if name != "chosen" {
                return false;
            }
            for (name_off, data) in props {
                if crate::libfdt::fdt::prop_name(*name_off) == "bootargs" {
                    let n = data.len().min(BUF_LEN);
                    src[..n].copy_from_slice(&data[..n]);
                    len = n;
                    return true;
                }
            }
            false
        });
        parse(&src[..len]);
    }
}

/// Tokenises `src`: whitespace-separated `key[=value]` entries copied inline
/// into [`BUF`]. # Safety
/// Caller must hold exclusive access to the statics (init runs once, before
/// secondary harts start logging).
unsafe fn parse(src: &[u8]) {
    unsafe {
        let mut i = 0usize;
        while i < src.len() {
            while i < src.len() && matches!(src[i], b' ' | b'\t' | b'\n' | b'\r' | 0) {
                i += 1;
            }
            let tok_start = i;
            while i < src.len() && !matches!(src[i], b' ' | b'\t' | b'\n' | b'\r' | 0) {
                i += 1;
            }
            if i == tok_start {
                continue;
            }
            let eq = src[tok_start..i]
                .iter()
                .position(|&b| b == b'=')
                .map_or(i, |off| tok_start + off);
            let n = N_ENTRIES;
            if n >= MAX_ENTRIES || BUF_USED + (i - tok_start) > BUF_LEN {
                return;
            }
            // Values live inline right after their key so one flat buffer holds
            // the whole command line; the only limit is BUF_LEN.
            let dst = BUF_USED;
            BUF[dst..dst + (i - tok_start)].copy_from_slice(&src[tok_start..i]);
            BUF_USED += i - tok_start;
            ENTRIES[n] = Entry {
                key: Range {
                    start: dst,
                    end: dst + (eq - tok_start),
                },
                val: Range {
                    start: dst + (eq - tok_start) + usize::from(eq < i),
                    end: dst + (i - tok_start),
                },
            };
            N_ENTRIES += 1;
        }
    }
}

/// Looks up `key` in the parsed command line. Returns the value slice for
/// `key=value`, an empty slice for a bare `key`, or `None` when absent.
pub fn get(key: &[u8]) -> Option<&'static [u8]> {
    unsafe {
        let buf = &BUF;
        let n = N_ENTRIES;
        let entries = &ENTRIES;
        for e in &entries[..n] {
            let k = &buf[e.key.start..e.key.end];
            if k == key {
                return Some(&buf[e.val.start..e.val.end]);
            }
        }
    }
    None
}

/// Applies `loglevel=` to the klog filter. Unknown or absent values keep the
/// default (`Info`, i.e. the historical behaviour of printing info and up).
pub fn apply_log_level() {
    let level = match get(b"loglevel") {
        Some(b"error") => Level::Err,
        Some(b"warn") => Level::Wrn,
        Some(b"info") => Level::Inf,
        _ => return,
    };
    klog::set_max_level(level);
}
