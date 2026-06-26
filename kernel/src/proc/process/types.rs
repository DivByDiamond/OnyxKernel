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
    pub fds: [crate::fs::vfs::VfsFd; PROC_MAX_FDS],
    pub next: *mut Proc,
    pub wait_next: *mut Proc,
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
            fds: [crate::fs::vfs::VfsFd {
                ino: 0,
                size: 0,
                pos: 0,
                fs: crate::fs::vfs::Fs::None,
                used: false,
                perms: 0,
                epoch: 0,
            }; PROC_MAX_FDS],
            next: ptr::null_mut(),
            wait_next: ptr::null_mut(),
        }
    }
}
