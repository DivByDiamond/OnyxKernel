/// Console input line discipline: applied to every byte pulled from the
/// UART before it reaches a reader of fd 0.
///
/// ETX (0x03) — what OC2R sends for Ctrl+C — never reaches the read buffer:
/// it is translated to SIGINT for the foreground process (the most recently
/// created non-init process, see proc::signals::G_FG_PID). Returns None for
/// consumed control bytes, Some(b) otherwise.
pub(crate) fn filter_input(b: u8) -> Option<u8> {
    if b == 0x03 {
        // SAFETY: signal_foreground validates the signal number and treats a
        // stale foreground pid as a no-op; no proc_list_lock is held here and
        // it takes that lock internally via by_pid.
        unsafe {
            let _ = crate::proc::signal_foreground(crate::proc::SIGINT);
        }
        return None;
    }
    Some(b)
}
