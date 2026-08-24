use crate::fs::vfs;
use crate::mm::vmm;
use crate::proc;
use onyx_core::errno::Errno;

use super::super::handler::{parse_user_path, user_ptr_ok};
use crate::syscall::abi::{CLOCK_MONOTONIC, CLOCK_REALTIME};

/// Copyout helper: validate + copy a fixed 16-byte `{u64, u64}` struct to
/// user memory, re-translating across page boundaries.
unsafe fn put_time_pair(out_va: u64, a: u64, b: u64) -> i64 {
    unsafe {
        if !user_ptr_ok(out_va, 16) {
            return Errno::Fault.as_i64();
        }
        let pair = [a, b];
        match vmm::copy_to_user(
            proc::current().root_pa,
            out_va,
            pair.as_ptr().cast::<u8>(),
            16,
        ) {
            Ok(()) => 0,
            Err(e) => e.as_i64(),
        }
    }
}

pub unsafe fn sys_gettimeofday(tv: u64) -> i64 {
    unsafe {
        let us = crate::srv::timer::uptime_us();
        put_time_pair(tv, us / 1_000_000, us % 1_000_000)
    }
}

pub unsafe fn sys_utimens(path: u64, times: u64) -> i64 {
    unsafe {
        let mut path_buf = [0u8; 256];
        let path_len = match parse_user_path(path, &mut path_buf) {
            Some(l) => l,
            None => return Errno::Inval.as_i64(),
        };
        let path_bytes = &path_buf[..path_len];
        if times == 0 {
            let now = crate::srv::timer::G_JIFFIES;
            return match vfs::utimens(path_bytes, now, now) {
                Ok(()) => 0,
                Err(e) => e.as_i64(),
            };
        }
        if !user_ptr_ok(times, 16) {
            return Errno::Fault.as_i64();
        }
        let mut t = [0u64; 2];
        if let Err(e) = vmm::copy_from_user(
            proc::current().root_pa,
            t.as_mut_ptr().cast::<u8>(),
            times,
            16,
        ) {
            return e.as_i64();
        }
        let (atime, mtime) = (t[0], t[1]);
        match vfs::utimens(path_bytes, mtime, atime) {
            Ok(()) => 0,
            Err(e) => e.as_i64(),
        }
    }
}

/// nanosleep — block (yielding the CPU) until at least `req.tv_sec*1e9 +
/// req.tv_nsec` nanoseconds have elapsed. The old implementation busy-looped
/// with `set_need_resched`, which burnt CPU; this version yields properly
/// while still polling `G_JIFFIES` from the timer tick.
pub unsafe fn sys_nanosleep(req: u64, _rem: u64) -> i64 {
    unsafe {
        if !user_ptr_ok(req, 16) {
            return Errno::Fault.as_i64();
        }
        let mut t = [0u64; 2];
        if let Err(e) = vmm::copy_from_user(
            proc::current().root_pa,
            t.as_mut_ptr().cast::<u8>(),
            req,
            16,
        ) {
            return e.as_i64();
        }
        let (secs, nsecs) = (t[0], t[1]);
        let total_ns = secs.saturating_mul(1_000_000_000).saturating_add(nsecs);
        let ticks = total_ns / 10_000_000; // 10 ms per tick
        let target = (crate::srv::timer::G_JIFFIES).wrapping_add(ticks.max(1));
        loop {
            let now = crate::srv::timer::G_JIFFIES;
            if now >= target {
                break;
            }
            // Wait for interrupt — the timer tick will wake us.  Between ticks
            // the CPU stays in a low-power state instead of busy-looping.
            #[cfg(not(test))]
            core::arch::asm!("wfi", options(nostack, preserves_flags));
            #[cfg(test)]
            core::hint::spin_loop();
        }
        0
    }
}

/// clock_gettime(clk_id, *ts) — POSIX clock query. Fills `ts` with
/// `{tv_sec, tv_nsec}`. CLOCK_REALTIME and CLOCK_MONOTONIC both return the
/// kernel uptime for now (no RTC synchronization yet).
pub unsafe fn sys_clock_gettime(clk_id: u64, ts: u64) -> i64 {
    unsafe {
        if !user_ptr_ok(ts, 16) {
            return Errno::Fault.as_i64();
        }
        match clk_id {
            CLOCK_REALTIME | CLOCK_MONOTONIC => {
                let us = crate::srv::timer::uptime_us();
                put_time_pair(ts, us / 1_000_000, (us % 1_000_000) * 1_000)
            }
            _ => Errno::Inval.as_i64(),
        }
    }
}

/// clock_getres(clk_id, *res) — resolution of the given clock. OnyxKernel's
/// timer ticks at 100 Hz (10 ms), so we report 10 ms for both clocks.
pub unsafe fn sys_clock_getres(clk_id: u64, res: u64) -> i64 {
    unsafe {
        if !user_ptr_ok(res, 16) {
            return Errno::Fault.as_i64();
        }
        match clk_id {
            CLOCK_REALTIME | CLOCK_MONOTONIC => put_time_pair(res, 0, 10_000_000),
            _ => Errno::Inval.as_i64(),
        }
    }
}
