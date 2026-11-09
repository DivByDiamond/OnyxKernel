//! Timekeeping: monotonic tick/uptime counters, per-hart timer arming and
//! the RTC-backed wall clock.

pub mod timer;
mod timer_arm;
pub mod wallclock;
