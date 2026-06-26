use crate::fs::vfs;
use onyx_core::errno::Errno;

use super::super::handler::{parse_user_path, user_ptr_ok};

pub unsafe fn sys_gettimeofday(tv: u64) -> i64 {
    if !user_ptr_ok(tv, 16) {
        return Errno::Inval.as_i64();
    }
    let us = crate::srv::timer::uptime_us();
    let secs = us / 1_000_000;
    let usecs = us % 1_000_000;
    let out = tv as *mut u64;
    *out = secs;
    *out.add(1) = usecs;
    0
}

pub unsafe fn sys_utimens(path: u64, times: u64) -> i64 {
    let path_bytes = match parse_user_path(path) {
        Some(s) => s,
        None => return Errno::Inval.as_i64(),
    };
    if times == 0 {
        let now = *(&raw const crate::srv::timer::G_JIFFIES);
        return match vfs::utimens(path_bytes, now, now) {
            Ok(()) => 0,
            Err(e) => e.as_i64(),
        };
    }
    if !user_ptr_ok(times, 16) {
        return Errno::Inval.as_i64();
    }
    let t = times as *const u64;
    let atime = *t;
    let mtime = *t.add(1);
    match vfs::utimens(path_bytes, mtime, atime) {
        Ok(()) => 0,
        Err(e) => e.as_i64(),
    }
}

pub unsafe fn sys_nanosleep(req: u64, _rem: u64) -> i64 {
    if !user_ptr_ok(req, 8) {
        return Errno::Inval.as_i64();
    }
    let ns = *(req as *const u64);
    let ticks = ns / 10_000_000;
    let target = (*(&raw const crate::srv::timer::G_JIFFIES)).wrapping_add(ticks);
    loop {
        let now = *(&raw const crate::srv::timer::G_JIFFIES);
        if now >= target {
            break;
        }
        crate::proc::set_need_resched(true);
    }
    0
}
