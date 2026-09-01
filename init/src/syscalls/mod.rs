// TODO(2026-08-27): shared auth/syscalls module, compiled per onyx_init bin via
// `mod syscalls;`. Wrapper fns unused by one bin are used by others; per-bin
// dead_code/unused_imports warnings are unavoidable without a lib target.
// Verified 2026-08-27: all wrappers are live (each reaches at least one bin,
// the rest form the init-side syscall library surface for upcoming bins).
#![allow(dead_code, unused_imports)]
pub mod call;
pub mod consts;

pub use call::comm::{
    chan_close, chan_connect, chan_create, chan_create_named, chan_open, chan_recv, chan_send,
    chmod, chown, fchmod, fchown, fsync, ftruncate, readlink, snapshot_create, snapshot_list,
    snapshot_rollback, symlink, truncate2, utimens,
};
pub use call::mouse_read;
pub use call::proc::{
    dropping, exec, execve, exit, fork, getpgid, getpid, getppid, getring, kill, setpgid, setsid,
    sigaction, sigmask, sigprocmask, sigreturn, spawn, wait, waitpid, yield_cpu,
};
pub use call::timer::{
    brk, clock_getres, clock_gettime, getentropy, getgid, gettimeofday, getuid, ioctl, isatty,
    mmap, mprotect, munmap, nanosleep, sbrk, setgid, setuid, umask, uname,
};
pub use call::{
    access, chdir, close, create, dup, fcntl, fstat, getcwd, getdents, getdents64, lseek, mkdir,
    open, pipe, poll, read, readdir, rename, stat, unlink, write, write_fd,
};
