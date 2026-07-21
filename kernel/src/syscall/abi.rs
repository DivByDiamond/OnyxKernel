#![allow(non_upper_case_globals)]

pub const SYS_write: u64 = 1;
pub const SYS_read: u64 = 2;
pub const SYS_exit: u64 = 3;
pub const SYS_yield: u64 = 4;
pub const SYS_getpid: u64 = 5;
pub const SYS_brk: u64 = 6;
pub const SYS_mmap: u64 = 7;
pub const SYS_open: u64 = 8;
pub const SYS_close: u64 = 9;
pub const SYS_lseek: u64 = 10;
pub const SYS_stat: u64 = 11;
pub const SYS_exec: u64 = 12;
pub const SYS_sbrk: u64 = 13;
pub const SYS_spawn: u64 = 14;
pub const SYS_wait: u64 = 15;
pub const SYS_readdir: u64 = 16;
pub const SYS_getring: u64 = 17;
pub const SYS_dropring: u64 = 18;
pub const SYS_snapshot_create: u64 = 19;
pub const SYS_snapshot_rollback: u64 = 20;
pub const SYS_snapshot_list: u64 = 21;
pub const SYS_kill: u64 = 22;
pub const SYS_sigmask: u64 = 23;
pub const SYS_write_fd: u64 = 24;
pub const SYS_create: u64 = 25;
pub const SYS_mkdir: u64 = 26;
pub const SYS_chan_create: u64 = 27;
pub const SYS_chan_connect: u64 = 28;
pub const SYS_chan_send: u64 = 29;
pub const SYS_chan_recv: u64 = 30;
pub const SYS_chan_close: u64 = 31;
pub const SYS_chan_create_named: u64 = 32;
pub const SYS_chan_open: u64 = 33;
pub const SYS_munmap: u64 = 34;
pub const SYS_dup: u64 = 35;
pub const SYS_pipe: u64 = 36;
pub const SYS_unlink: u64 = 37;
pub const SYS_rename: u64 = 38;
pub const SYS_chdir: u64 = 39;
pub const SYS_getcwd: u64 = 40;
pub const SYS_truncate: u64 = 41;
pub const SYS_access: u64 = 42;
pub const SYS_gettimeofday: u64 = 43;
pub const SYS_fcntl: u64 = 44;
pub const SYS_getuid: u64 = 45;
pub const SYS_getgid: u64 = 46;
pub const SYS_utimens: u64 = 47;
pub const SYS_uname: u64 = 48;
pub const SYS_nanosleep: u64 = 49;

// ── New syscalls added in the v0.4 "userspace readiness" update ─────────
// These close the gap between OnyxKernel and a typical libc/libonyxc so that
// user programs compiled by OnyxCC can run without kernel-side workarounds.
pub const SYS_fstat: u64 = 50; // fstat(fd, struct stat *)
pub const SYS_waitpid: u64 = 51; // waitpid(pid, *status, options)
pub const SYS_getdents64: u64 = 52; // getdents64(fd, *buf, len)
pub const SYS_ioctl: u64 = 53; // ioctl(fd, req, arg)
pub const SYS_mprotect: u64 = 54; // mprotect(addr, len, prot)
pub const SYS_sigaction: u64 = 55; // sigaction(signum, *act, *oldact)
pub const SYS_sigprocmask: u64 = 56; // sigprocmask(how, *set, *oldset)
pub const SYS_sigreturn: u64 = 57; // sigreturn() — restore tf from signal frame
pub const SYS_execve: u64 = 58; // execve(path, argv, envp) — exec with envp
pub const SYS_getppid: u64 = 59; // getppid()
pub const SYS_setpgid: u64 = 60; // setpgid(pid, pgid)
pub const SYS_setsid: u64 = 61; // setsid()
pub const SYS_getpgid: u64 = 62; // getpgid(pid)
pub const SYS_fork: u64 = 63; // fork() — duplicate current process (vfork-style)
pub const SYS_clock_gettime: u64 = 64; // clock_gettime(clk_id, *ts)
pub const SYS_clock_getres: u64 = 65; // clock_getres(clk_id, *res)
pub const SYS_isatty: u64 = 66; // isatty(fd)
pub const SYS_getentropy: u64 = 67; // getentropy(buf, len)
pub const SYS_setuid: u64 = 68; // setuid(uid)  (root only)
pub const SYS_setgid: u64 = 69; // setgid(gid)  (root only)
pub const SYS_fsync: u64 = 70; // fsync(fd)
pub const SYS_truncate2: u64 = 71; // truncate(path, length) — fixed signature
pub const SYS_ftruncate: u64 = 72; // ftruncate(fd, length)
pub const SYS_readlink: u64 = 73; // readlink(path, buf, len)
pub const SYS_symlink: u64 = 74; // symlink(target, linkpath)
pub const SYS_chmod: u64 = 75; // chmod(path, mode)
pub const SYS_fchmod: u64 = 76; // fchmod(fd, mode)
pub const SYS_getdents: u64 = 77; // getdents(fd, *buf, len)  (compat alias)
pub const SYS_sched_setaffinity: u64 = 78; // sched_setaffinity(pid, cpu) — pin to CPU
pub const SYS_sched_getaffinity: u64 = 79; // sched_getaffinity(pid) -> cpu (or -1 for any)
pub const SYS_net_connect: u64 = 80; // net_connect(ip_ptr, port) -> conn_id
pub const SYS_net_send: u64 = 81; // net_send(conn_id, buf, len) -> bytes
pub const SYS_net_recv: u64 = 82; // net_recv(conn_id, buf, len) -> bytes
pub const SYS_net_close: u64 = 83; // net_close(conn_id) -> 0

// ── Multi-user syscalls (84–85) ──────────────────────────────────────────
pub const SYS_chown: u64 = 84; // chown(path, uid, gid)
pub const SYS_fchown: u64 = 85; // fchown(fd, uid, gid)

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
