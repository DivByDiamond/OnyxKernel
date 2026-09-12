//! Core scheduling entry points: tick/resched-flag bookkeeping, work
//! stealing, and the yield/context-switch path. Split out of a single
//! `sched.rs` (SRP, info.md 250-line rule) into per-responsibility modules;
//! the public API below is unchanged.

mod steal;
mod tick;
mod r#yield;

pub use steal::steal;
pub use tick::{sched_tick, set_need_resched};
pub use r#yield::sched_yield;
