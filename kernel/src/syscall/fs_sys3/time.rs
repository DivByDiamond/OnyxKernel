use crate::fs::vfs;
use crate::mm::vmm;
use crate::proc;
use onyx_core::errno::Errno;

use super::super::handler::{parse_user_path, user_ptr_ok};
use crate::syscall::abi::{CLOCK_MONOTONIC, CLOCK_REALTIME};

/// Copyout helper: validate + copy a fixed 16-byte `{u64, u64}` struct to
/// user memory, re-translating across page boundaries.
/// # Safety
///
/// Caller must be on the syscall path with a live current-process root_pa;
/// `out_va` is an untrusted user address validated here before any write.
unsafe fn put_time_pair(out_va: u64, a: u64, b: u64) -> i64 {
    // SAFETY: out_va passed user_ptr_ok (16 bytes) above and copy_to_user
    // re-validates each page as a writable user mapping before writing the
    // 16-byte stack pair.
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

/// # Safety
///
/// Call only from the syscall path with a current process set; `tv` is
/// validated inside put_time_pair before any write.
pub unsafe fn sys_gettimeofday(tv: u64) -> i64 {
    // SAFETY: the only unsafe operation is put_time_pair, which validates
    // `tv` (range + per-page mapping) internally; uptime_us is a safe timer
    // read.
    unsafe {
        let us = crate::srv::timer::uptime_us();
        put_time_pair(tv, us / 1_000_000, us % 1_000_000)
    }
}

/// # Safety
///
/// Call only from handler::handle's syscall path: current process set, ACL
/// checked; `path` and `times` are validated inside before use.
pub unsafe fn sys_utimens(path: u64, times: u64) -> i64 {
    // SAFETY: parse_user_path validates the path internally; the 16-byte
    // times range passed user_ptr_ok above and copy_from_user re-validates
    // each page before reading into the kernel stack array.
    unsafe {
        let mut path_buf = [0u8; 256];
        let path_len = match parse_user_path(path, &mut path_buf) {
            Some(l) => l,
            None => return Errno::Inval.as_i64(),
        };
        let path_bytes = &path_buf[..path_len];
        if times == 0 {
            let now = crate::srv::timer::jiffies();
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

/// nanosleep — block until at least `req.tv_sec*1e9 + req.tv_nsec`
/// nanoseconds have elapsed, polling the timer tick counter
/// (`timer::jiffies`) between `wfi` waits.
///
/// The original implementation `wfi`-looped without ever setting
/// `sstatus.SIE`, so the timer tick that advances `jiffies` could never be
/// delivered — this was the root cause of `usleep()`/`nanosleep()` hanging
/// forever (see OnyxKernel/todo.md's 2026-09-02 entry for the full
/// multi-attempt investigation). Live QEMU debugging (`info registers` over
/// the monitor while hung) traced it one level deeper: even with SIE set
/// and the comparator freshly re-armed, `wfi` still never woke, because the
/// CLINT `mtimecmp` MMIO register this hart's timer used to be armed
/// through only ever asserts `mip.MTIP` — an M-mode-only interrupt RISC-V's
/// `mideleg` cannot delegate to S-mode, and this kernel's M→S boot
/// transition never returns to M-mode to forward it. The actual fix lives
/// in `srv::timer`/`arch::asm::mtrap`: a minimal M-mode trap vector now
/// forwards the machine timer interrupt to S-mode as `STIP` (see
/// `srv::timer::arm_timer`'s doc comment). With that fixed, this loop's
/// technique — mirroring `proc::scheduler::idle::sched_enter_idle`: re-arm
/// via `timer::init_hart`, set `sstatus.SIE`, `wfi` — actually wakes. No
/// lock is held across the wait (the only prior work was copying `req`
/// into a kernel stack array), matching the idle loop's audited "no lock
/// held" invariant for toggling SIE outside of `trap_return`'s two blessed
/// points (return-to-user, idle loop). When the timer interrupt fires,
/// hardware traps into `trap_entry` (nested inside this syscall's own
/// trap), `timer::handle()` runs and increments `jiffies`/re-arms the
/// comparator, and `trap_return` resumes execution right after `wfi` with
/// SIE cleared again.
/// # Safety
///
/// Call only from the syscall path with a current process set; `req` is
/// validated inside before use and `_rem` is never written.
pub unsafe fn sys_nanosleep(req: u64, _rem: u64) -> i64 {
    // SAFETY: the 16-byte req range passed user_ptr_ok above and
    // copy_from_user re-validates each page before reading into the kernel
    // stack array; no lock is held across the wait loop, matching the idle
    // loop's audited SIE/wfi invariant (timer::init_hart + set_sstatus
    // immediately before wfi, with the interrupt trap clearing SIE again on
    // entry and trap_return restoring it cleared on the way back here).
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
        let target = crate::srv::timer::jiffies().wrapping_add(ticks.max(1));
        while crate::srv::timer::jiffies() < target {
            #[cfg(not(test))]
            {
                crate::srv::timer::init_hart(crate::proc::process::hart_id());
                crate::arch::csr::set_sstatus(crate::arch::regs::SSTATUS_SIE);
                crate::arch::csr::wfi();
            }
            #[cfg(test)]
            core::hint::spin_loop();
        }
        0
    }
}

/// clock_gettime(clk_id, *ts) — POSIX clock query. Fills `ts` with
/// `{tv_sec, tv_nsec}`. CLOCK_REALTIME and CLOCK_MONOTONIC both return the
/// kernel uptime for now (no RTC synchronization yet).
/// # Safety
///
/// Call only from the syscall path with a current process set; `ts` is
/// validated inside before any write.
pub unsafe fn sys_clock_gettime(clk_id: u64, ts: u64) -> i64 {
    // SAFETY: the only unsafe operation is put_time_pair, which validates
    // `ts` (range + per-page mapping) internally; uptime_us is a safe timer
    // read.
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
/// # Safety
///
/// Call only from the syscall path with a current process set; `res` is
/// validated inside before any write.
pub unsafe fn sys_clock_getres(clk_id: u64, res: u64) -> i64 {
    // SAFETY: the only unsafe operation is put_time_pair, which validates
    // `res` (range + per-page mapping) internally.
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
