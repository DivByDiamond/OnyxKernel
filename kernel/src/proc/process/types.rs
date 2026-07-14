use crate::arch::trap_frame::TrapFrame;
use core::ptr;

pub const PROC_RING_KERNEL: u8 = 0;
pub const PROC_RING_ROOT: u8 = 1;
pub const PROC_RING_USER: u8 = 2;

pub const PROC_PID_INIT: u32 = 1;
pub const KSTACK_SIZE: usize = 16 * 1024;
pub const PROC_MAX_FDS: usize = 16;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProcState {
    Free = 0,
    Ready = 1,
    Running = 2,
    Exited = 3,
    Waiting = 4,
}

#[repr(C, align(16))]
pub struct Proc {
    pub pid: u32,
    pub ring: u8,
    pub state: ProcState,
    pub parent_pid: u32,
    pub exit_code: i32,
    pub root_pa: u64,
    pub entry: u64,
    pub ustack: u64,
    pub heap_brk: u64,
    pub mmap_brk: u64,
    pub cwd: [u8; 256],
    pub cwd_len: u16,
    pub uid: u32,
    pub gid: u32,
    pub tf: TrapFrame,
    pub kstack: [u8; KSTACK_SIZE],
    pub pending_signals: u32,
    pub signal_mask: u32,
    /// Per-signal user handler entry points (0 = default action).
    /// Indexed by signal number 1..NSIG. Signal 0 is unused (POSIX reserves
    /// it as "no signal"). Length is 32 to match NSIG.
    pub signal_handlers: [u64; 32],
    /// Per-signal sa_mask: signals to block while this handler runs.
    /// Applied transiently in signal_check, removed in sigreturn.
    pub signal_handler_masks: [u32; 32],
    /// Saved signal_mask for sigreturn. When a handler is entered, the
    /// current mask is stashed here so sigreturn can restore it.
    pub saved_mask: u32,
    /// Saved trap frame for `sigreturn`. When a signal handler is entered,
    /// the original user tf is stashed here so the `sigreturn` syscall can
    /// restore it. Single-deep — nested signals are not yet supported.
    pub saved_tf: TrapFrame,
    /// True if a signal handler is currently executing for this process.
    pub in_signal_handler: bool,
    pub fds: [crate::fs::vfs::VfsFd; PROC_MAX_FDS],
    pub readdir_ino: u32,
    pub readdir_idx: u32,
    pub readdir_active: bool,
    pub readdir_fs: crate::fs::vfs::Fs,
    pub root_refcount: *mut u32,
    pub all_next: *mut Proc,
    pub next: *mut Proc,
    pub wait_next: *mut Proc,
    pub affinity: i32,
    pub on_rq: bool,
    /// If true, sys_read(fd=0) returns raw bytes without line editing
    /// (no echo, no backspace handling, no Enter translation). Used by
    /// the shell for tab completion and arrow-key history navigation.
    /// Set via ioctl(TIOCSRAW), cleared via ioctl(TIOCRRAW).
    pub raw_stdin: bool,
}

impl Proc {
    #[expect(dead_code)]
    const fn new() -> Self {
        Self {
            pid: 0,
            ring: PROC_RING_KERNEL,
            state: ProcState::Free,
            parent_pid: 0,
            exit_code: 0,
            root_pa: 0,
            entry: 0,
            ustack: 0,
            heap_brk: 0,
            mmap_brk: 0x3000_0000,
            cwd: [0; 256],
            cwd_len: 0,
            uid: 0,
            gid: 0,
            tf: TrapFrame::zero(),
            kstack: [0; KSTACK_SIZE],
            pending_signals: 0,
            signal_mask: 0,
            signal_handlers: [0; 32],
            signal_handler_masks: [0; 32],
            saved_mask: 0,
            saved_tf: TrapFrame::zero(),
            in_signal_handler: false,
            fds: [crate::fs::vfs::VfsFd {
                ino: 0,
                size: 0,
                pos: 0,
                fs: crate::fs::vfs::Fs::None,
                used: false,
                perms: 0,
                epoch: 0,
                cloexec: false,
            }; PROC_MAX_FDS],
            root_refcount: ptr::null_mut(),
            all_next: ptr::null_mut(),
            next: ptr::null_mut(),
            wait_next: ptr::null_mut(),
            affinity: -1,
            on_rq: false,
            raw_stdin: false,
            readdir_ino: 0,
            readdir_idx: 0,
            readdir_active: false,
            readdir_fs: crate::fs::vfs::Fs::None,
        }
    }
}
