use super::*;
use std::sync::Mutex;

/// These tests flip the shared base statics; serialize them the same way
/// `net::lock::test_serial()` does for the network tables.
static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(|e| e.into_inner())
}

#[test]
fn realtime_falls_back_to_uptime_without_an_rtc_base() {
    let _serial = serial();
    // A fresh (or previously reset) base must not invent a wall clock.
    G_HAVE_RTC.store(false, Ordering::Release);
    assert_eq!(realtime_us(), uptime_us());
    assert!(!have_rtc());
}

#[test]
fn set_rtc_base_zero_is_ignored() {
    let _serial = serial();
    G_HAVE_RTC.store(false, Ordering::Release);
    set_rtc_base(0);
    assert!(!have_rtc());
    assert_eq!(realtime_us(), uptime_us());
}

#[test]
fn a_probed_base_shifts_uptime_to_the_epoch() {
    let _serial = serial();
    G_HAVE_RTC.store(false, Ordering::Release);
    // Host tests never tick, so uptime_us() == 0 and realtime must equal
    // the RTC reading exactly.
    set_rtc_base(1_762_000_000_000_000_000); // 2025-11-01, ns
    assert!(have_rtc());
    assert_eq!(realtime_us(), 1_762_000_000_000_000_000 / 1000);
    G_HAVE_RTC.store(false, Ordering::Release);
}

#[test]
fn split_offset_handles_positive_negative_and_rejects_overflow() {
    assert_eq!(split_offset(0), Some((0, 0)));
    assert_eq!(split_offset(1_500_000), Some((1, 500_000)));
    // Negative offsets (RTC behind uptime) round toward -1s with a
    // positive remainder, so reconstruction is exact.
    assert_eq!(split_offset(-1), Some((-1, 999_999)));
    assert_eq!(split_offset(-1_000_000), Some((-1, 0)));
    // Beyond i32 seconds (year 2038 in either direction) — rejected.
    assert_eq!(split_offset((i32::MAX as i64 + 1) * 1_000_000), None);
    assert_eq!(split_offset((i32::MIN as i64 - 1) * 1_000_000), None);
}

#[test]
fn offset_round_trips_exactly() {
    for off in [-5_000_000i64, -1, 0, 1, 999_999, 1_762_000_000_000] {
        let (s, u) = split_offset(off).expect("in range");
        assert_eq!(i64::from(s) * 1_000_000 + i64::from(u), off);
    }
}
