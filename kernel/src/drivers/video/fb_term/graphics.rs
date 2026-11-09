//! Voluntary primary-display ownership for native graphical applications.
//! UART remains available; the pre-existing console back buffer is restored
//! after release. The exiting process also releases its claim after a signal.
use core::sync::atomic::{AtomicU32, Ordering};
use onyx_core::errno::{Errno, KResult};

struct Owner(AtomicU32);
impl Owner {
    const fn new() -> Self {
        Self(AtomicU32::new(0))
    }
    fn claim(&self, pid: u32) -> KResult<()> {
        if pid == 0 {
            return Err(Errno::Inval);
        }
        match self
            .0
            .compare_exchange(0, pid, Ordering::AcqRel, Ordering::Acquire)
        {
            Ok(_) => Ok(()),
            Err(current) if current == pid => Ok(()),
            Err(_) => Err(Errno::Busy),
        }
    }
    fn release(&self, pid: u32) -> bool {
        pid != 0
            && self
                .0
                .compare_exchange(pid, 0, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
    }
}
static OWNER: Owner = Owner::new();

pub fn active() -> bool {
    OWNER.0.load(Ordering::Acquire) != 0
}

pub fn claim(pid: u32) -> KResult<()> {
    // Match console writers/presenters so no in-progress console write can
    // land after this call returns and the application starts painting.
    crate::srv::klog::UART_LOCK.lock();
    let result = OWNER.claim(pid);
    crate::srv::klog::UART_LOCK.unlock();
    result
}

pub fn release(pid: u32) -> bool {
    crate::srv::klog::UART_LOCK.lock();
    let released = OWNER.release(pid);
    if released {
        if !crate::drivers::fb::double_buffered() {
            crate::drivers::fb::clear();
        }
        super::ansi::console_dirty();
    }
    crate::srv::klog::UART_LOCK.unlock();
    released
}

#[cfg(test)]
mod tests;
