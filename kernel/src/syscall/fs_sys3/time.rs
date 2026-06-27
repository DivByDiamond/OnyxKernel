use crate::fs::vfs;
use onyx_core::errno::Errno;

use super::super::handler::{parse_user_path, user_ptr_ok};
use crate::syscall::abi::{CLOCK_MONOTONIC, CLOCK_REALTIME};

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

/// nanosleep — block (yielding the CPU) until at least `req.tv_sec*1e9 +
/// req.tv_nsec` nanoseconds have elapsed. The old implementation busy-looped
/// with `set_need_resched`, which burnt CPU; this version yields properly
/// while still polling `G_JIFFIES` from the timer tick.
pub unsafe fn sys_nanosleep(req: u64, _rem: u64) -> i64 {
    if !user_ptr_ok(req, 16) {
        return Errno::Inval.as_i64();
    }
    let t = req as *const u64;
    let secs = *t;
    let nsecs = *t.add(1);
    let total_ns = secs.saturating_mul(1_000_000_000).saturating_add(nsecs);
    let ticks = total_ns / 10_000_000; // 10 ms per tick
    let target = (*(&raw const crate::srv::timer::G_JIFFIES)).wrapping_add(ticks.max(1));
    loop {
        let now = *(&raw const crate::srv::timer::G_JIFFIES);
        if now >= target {
            break;
        }
        // Yield CPU instead of busy-looping. The scheduler will pick another
        // runnable process (or idle) until the next timer tick wakes us.
        crate::proc::set_need_resched(true);
        // Force a scheduler check by re-reading the jiffies — the trap-return
        // path will switch context if NEED_RESCHED is set.
        core::hint::spin_loop();
    }
    0
}

/// clock_gettime(clk_id, *ts) — POSIX clock query. Fills `ts` with
/// `{tv_sec, tv_nsec}`. CLOCK_REALTIME and CLOCK_MONOTONIC both return the
/// kernel uptime for now (no RTC synchronization yet).
pub unsafe fn sys_clock_gettime(clk_id: u64, ts: u64) -> i64 {
    if !user_ptr_ok(ts, 16) {
        return Errno::Inval.as_i64();
    }
    match clk_id {
        CLOCK_REALTIME | CLOCK_MONOTONIC => {
            let us = crate::srv::timer::uptime_us();
            let secs = us / 1_000_000;
            let nsecs = (us % 1_000_000) * 1_000;
            let out = ts as *mut u64;
            *out = secs;
            *out.add(1) = nsecs;
            0
        }
        _ => Errno::Inval.as_i64(),
    }
}

/// clock_getres(clk_id, *res) — resolution of the given clock. OnyxKernel's
/// timer ticks at 100 Hz (10 ms), so we report 10 ms for both clocks.
pub unsafe fn sys_clock_getres(clk_id: u64, res: u64) -> i64 {
    if !user_ptr_ok(res, 16) {
        return Errno::Inval.as_i64();
    }
    match clk_id {
        CLOCK_REALTIME | CLOCK_MONOTONIC => {
            let out = res as *mut u64;
            *out = 0;            // tv_sec = 0
            *out.add(1) = 10_000_000; // tv_nsec = 10 ms
            0
        }
        _ => Errno::Inval.as_i64(),
    }
}
