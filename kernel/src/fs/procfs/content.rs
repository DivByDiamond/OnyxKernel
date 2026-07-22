use crate::mm::{heap, pmm};
use crate::module;
use crate::proc;
use crate::srv::timer;
use onyx_core::errno::{Errno, KResult};

use super::consts::*;
use super::fmt;

pub unsafe fn read(ino: u32, buf: *mut u8, offset: u32, len: u32) -> KResult<u32> {
    let content = generate_content(ino)?;
    let avail = (content.len() as u32).saturating_sub(offset);
    let to_copy = len.min(avail) as usize;
    if to_copy > 0 {
        core::ptr::copy_nonoverlapping(content.as_ptr().add(offset as usize), buf, to_copy);
    }
    Ok(to_copy as u32)
}

unsafe fn generate_content(ino: u32) -> KResult<&'static [u8]> {
    static mut G_PROCBUF: [u8; PROCFS_MAX_SIZE as usize] = [0; PROCFS_MAX_SIZE as usize];
    let pb = &raw mut G_PROCBUF;
    let s = match ino {
        PROCFS_VERSION_INO => VERSION_STR,
        PROCFS_CPUINFO_INO => {
            let harts = crate::arch::smp::online_harts() as u64;
            let buf = &mut *pb;
            let mut pos = 0;
            pos += fmt::format_line(b"harts\t\t: ", harts, b"\n", buf, pos);
            for h in 0..harts {
                pos += fmt::format_line(b"processor\t: ", h, b"\n", buf, pos);
                if pos >= buf.len() {
                    break;
                }
            }
            pos += fmt::format_line_raw(b"model name\t: rv64gc\n", buf, pos);
            pos += fmt::format_line_raw(b"model\t\t: rv64gc\n", buf, pos);
            pos += fmt::format_line_raw(b"frequency\t: ~10 MHz (QEMU)\n", buf, pos);
            pos += fmt::format_line_raw(b"isa\t\t: rv64imafdc\n", buf, pos);
            core::str::from_utf8_unchecked(&buf[..pos.min(buf.len())])
        }
        PROCFS_MEMINFO_INO => {
            let buf = &mut *pb;
            let mut pos = 0;
            let total_pages = pmm::total_pages();
            let free_pages = pmm::free_pages();
            let used_pages = total_pages - free_pages;
            let total_kb = (total_pages * 4) as u64;
            let free_kb = (free_pages * 4) as u64;
            let used_kb = (used_pages * 4) as u64;
            let heap_used = heap::used();
            pos += fmt::format_line(b"MemTotal\t: ", total_kb, b" kB\n", buf, pos);
            pos += fmt::format_line(b"MemFree\t\t: ", free_kb, b" kB\n", buf, pos);
            pos += fmt::format_line(b"MemUsed\t\t: ", used_kb, b" kB\n", buf, pos);
            pos += fmt::format_line(b"HeapTotal\t: 4096 kB\n", 0, b"", buf, pos);
            pos += fmt::format_line(
                b"HeapUsed\t: ",
                (heap_used / 1024) as u64,
                b" kB\n",
                buf,
                pos,
            );
            let heap_free_kb = 4096u64.saturating_sub(heap_used as u64 / 1024);
            pos += fmt::format_line(b"HeapFree\t: ", heap_free_kb, b" kB\n", buf, pos);
            core::str::from_utf8_unchecked(&buf[..pos.min(buf.len())])
        }
        PROCFS_UPTIME_INO => {
            let buf = &mut *pb;
            let mut pos = 0;
            let us = timer::uptime_us();
            let secs = us / 1_000_000;
            let frac = (us % 1_000_000) / 10_000;
            pos += fmt::format_dec(secs, buf, pos);
            pos += b".".len();
            if pos + 2 <= buf.len() {
                buf[pos] = b'0' + (frac / 10) as u8;
                buf[pos + 1] = b'0' + (frac % 10) as u8;
                pos += 2;
            }
            pos += fmt::format_line_raw(b"\n", buf, pos);
            core::str::from_utf8_unchecked(&buf[..pos.min(buf.len())])
        }
        PROCFS_LOAD_INO => {
            let buf = &mut *pb;
            let mut pos = 0;
            let procs = proc::count();
            pos += fmt::format_line(b"processes\t: ", procs as u64, b"\n", buf, pos);
            let running = 1u64;
            pos += fmt::format_line(b"procs_running\t: ", running, b"\n", buf, pos);
            pos += fmt::format_line(b"procs_blocked\t: 0\n", 0, b"", buf, pos);
            core::str::from_utf8_unchecked(&buf[..pos.min(buf.len())])
        }
        PROCFS_STAT_INO => {
            let buf = &mut *pb;
            let mut pos = 0;
            let us = timer::uptime_us();
            let secs = us / 1_000_000;
            let _harts = crate::arch::smp::online_harts();
            let procs = proc::count();
            pos += fmt::format_line(b"btime ", secs, b"\n", buf, pos);
            pos += fmt::format_line(b"cpu 0 0 0 0 0 0 0 0 0 0\n", 0, b"", buf, pos);
            pos += fmt::format_line(b"processes ", procs as u64, b"\n", buf, pos);
            pos += fmt::format_line(b"procs_running 1\n", 0, b"", buf, pos);
            pos += fmt::format_line(b"procs_blocked 0\n", 0, b"", buf, pos);
            core::str::from_utf8_unchecked(&buf[..pos.min(buf.len())])
        }
        PROCFS_MODULES_INO => {
            let buf = &mut *pb;
            let len = module::format_list(buf);
            core::str::from_utf8_unchecked(&buf[..len.min(buf.len())])
        }
        _ => return Err(Errno::NoEnt),
    };
    Ok(s.as_bytes())
}
