//! PTY device nodes (/dev/ptmx, /dev/pts/N) and the winsize/ptn ioctls
//! they support.

pub(super) mod pty_ioctl;
pub(super) mod pty_nodes;
