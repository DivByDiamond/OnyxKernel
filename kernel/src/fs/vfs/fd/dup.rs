use crate::fs::vfs::{
    FdToken, Fs, PERM_READ, PERM_WRITE, alloc_fd, fd_check, fd_get, fd_set, fd_set_flags, fd_token,
};
use onyx_core::errno::KResult;

/// # Safety
///
/// Caller contract: token must be a live fd token of the calling context;
/// dup only clones into a fresh slot of the same context's fd table.
pub unsafe fn dup(token: FdToken) -> KResult<FdToken> {
    // SAFETY: fd_check validates idx/epoch; alloc_fd claims a fresh slot in
    // this context and fd_set fills exactly that slot.
    unsafe {
        let idx = fd_check(token)?;
        let fd = fd_get(idx);
        let new_idx = alloc_fd(fd.perms)?;
        fd_set(new_idx, fd.ino, fd.size, fd.fs, fd.pos);
        // Status flags (O_NONBLOCK/O_APPEND/...) belong to the open file
        // description semantics-wise; our fd table has no shared-OFD concept,
        // so dup copies them into the new slot (POSIX-adjacent behavior).
        fd_set_flags(new_idx, fd.flags);
        let new_fd = fd_get(new_idx);
        Ok(fd_token(new_idx, new_fd.epoch))
    }
}

/// # Safety
///
/// Caller contract: runs in the fd-owning context (kernel boot or the
/// current process's syscall on this hart); allocates two slots plus one
/// IPC channel, all owned by this context.
pub unsafe fn create_pipe() -> KResult<(FdToken, FdToken)> {
    // SAFETY: r_idx/w_idx come from alloc_fd in this context, so the direct
    // G_KERNEL_FDS / p.fds writes below touch only slots this context owns
    // (same contract as fd_set in ops.rs).
    unsafe {
        let r_idx = alloc_fd(PERM_READ)?;
        let w_idx = alloc_fd(PERM_WRITE)?;
        // Bug #24 fix: previously used pipe_ino = !0u32 (0xFFFFFFFF). When a
        // pipe fd was later read/written, ipcfs::read/write computed
        // chan_id = ino - 2 = 0xFFFFFFFD and indexed G_CHANNELS[0xFFFFFFFD],
        // a massive OOB write into kernel memory. Now we allocate a real IPC
        // channel and use (chan_id + 2) as the ino, matching the ipcfs
        // convention (ino 0/1 are reserved, ino 2+ maps to chan_id = ino-2).
        let owner_pid = if crate::fs::vfs::ops::is_kernel_boot() {
            0
        } else {
            crate::proc::current_pid()
        };
        let chan_id = crate::ipc::create(owner_pid)?;
        let pipe_ino = chan_id + 2;
        if crate::fs::vfs::ops::is_kernel_boot() {
            let p = &raw mut crate::fs::vfs::ops::G_KERNEL_FDS;
            (*p)[r_idx].ino = pipe_ino;
            (*p)[r_idx].size = 0;
            (*p)[r_idx].fs = Fs::Ipc;
            (*p)[r_idx].pos = 0;
            (*p)[w_idx].ino = pipe_ino;
            (*p)[w_idx].size = 0;
            (*p)[w_idx].fs = Fs::Ipc;
            (*p)[w_idx].pos = 0;
        } else {
            let p = crate::proc::current();
            p.fds[r_idx].ino = pipe_ino;
            p.fds[r_idx].size = 0;
            p.fds[r_idx].fs = Fs::Ipc;
            p.fds[r_idx].pos = 0;
            p.fds[w_idx].ino = pipe_ino;
            p.fds[w_idx].size = 0;
            p.fds[w_idx].fs = Fs::Ipc;
            p.fds[w_idx].pos = 0;
        }
        let r_fd = fd_get(r_idx);
        let w_fd = fd_get(w_idx);
        Ok((fd_token(r_idx, r_fd.epoch), fd_token(w_idx, w_fd.epoch)))
    }
}
