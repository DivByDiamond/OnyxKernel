#![allow(
    non_upper_case_globals,
    reason = "SYS_* constants mirror Linux/kernel ABI syscall-name casing (e.g. SYS_write), deliberately non-upper-case by convention"
)]

include!("numbers.rs");

// ── Flags / constants used by syscalls ─────────────────────────────────

pub const SEEK_SET: u32 = 0;
pub const SEEK_CUR: u32 = 1;
pub const SEEK_END: u32 = 2;

// open() flags — Linux-compatible low bits.
pub const O_RDONLY: u32 = 0;
pub const O_WRONLY: u32 = 1;
pub const O_RDWR: u32 = 2;
pub const O_ACCMODE: u32 = 3;
pub const O_CREAT: u32 = 1 << 6; // 0x40
pub const O_EXCL: u32 = 1 << 7; // 0x80
pub const O_TRUNC: u32 = 1 << 9; // 0x200
pub const O_APPEND: u32 = 1 << 10; // 0x400
pub const O_NONBLOCK: u32 = 1 << 11; // 0x800
pub const O_DIRECTORY: u32 = 1 << 16; // 0x10000

// fcntl() commands.
pub const F_DUPFD: u32 = 0;
/// Onyx extension: bind fd to standard stream arg (0..2); MAX fd restores UART.
pub const F_BIND_STDIO: u32 = 0x4f01;
pub const F_GETFD: u32 = 1;
pub const F_SETFD: u32 = 2;
pub const F_GETFL: u32 = 3;
pub const F_SETFL: u32 = 4;
pub const FD_CLOEXEC: u32 = 1;

// ioctl() requests (subset).
pub const TCGETS: u64 = 0x5401;
pub const TCSETS: u64 = 0x5402;

// waitpid() options.
pub const WNOHANG: u32 = 1;
pub const WUNTRACED: u32 = 2;

// poll() events/revents bits (Linux values).
pub const POLLIN: i32 = 0x001;
pub const POLLOUT: i32 = 0x004;
pub const POLLERR: i32 = 0x008;
pub const POLLNVAL: i32 = 0x020;

/// Onyx `struct pollfd`. Onyx fds are 64-bit (idx, epoch) tokens — unlike
/// Linux's 32-bit ints — so `fd` is widened to i64; `events`/`revents` are
/// widened to i32 so the struct is 16 bytes with no padding. The userspace
/// mirror lives in libonyxc include/io/poll.h.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PollFd {
    pub fd: i64,
    pub events: i32,
    pub revents: i32,
}

/// poll() argument cap: bounds the kernel-side PollFd staging buffer.
pub const POLL_MAX_FDS: u64 = 32;

impl PollFd {
    pub const fn zeroed() -> Self {
        Self {
            fd: 0,
            events: 0,
            revents: 0,
        }
    }
}

// sigaction / sigprocmask `how`.
pub const SIG_BLOCK: u32 = 0;
pub const SIG_UNBLOCK: u32 = 1;
pub const SIG_SETMASK: u32 = 2;

// Standard POSIX signals.
pub const SIGHUP: u32 = 1;
pub const SIGINT: u32 = 2;
pub const SIGQUIT: u32 = 3;
pub const SIGILL: u32 = 4;
pub const SIGTRAP: u32 = 5;
pub const SIGABRT: u32 = 6;
pub const SIGBUS: u32 = 7;
pub const SIGFPE: u32 = 8;
pub const SIGKILL: u32 = 9;
pub const SIGUSR1: u32 = 10;
pub const SIGSEGV: u32 = 11;
pub const SIGUSR2: u32 = 12;
pub const SIGPIPE: u32 = 13;
pub const SIGALRM: u32 = 14;
pub const SIGTERM: u32 = 15;
pub const SIGCHLD: u32 = 17;
pub const SIGCONT: u32 = 18;
pub const SIGSTOP: u32 = 19;
pub const SIGTSTP: u32 = 20;
pub const NSIG: u32 = 32;

// clock_gettime() clocks.
pub const CLOCK_REALTIME: u64 = 0;
pub const CLOCK_MONOTONIC: u64 = 1;

pub const RING_KERNEL: u64 = 0;
pub const RING_ROOT: u64 = 1;
pub const RING_USER: u64 = 2;
