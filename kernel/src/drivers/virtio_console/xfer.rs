//! Nonblocking byte-stream I/O. Buffer ownership remains with the device
//! until its used index advances; no per-byte page allocation or busy wait.
use super::{BUF_SIZE, QUEUE_SIZE, G_CON, LOCK, present, push};
use crate::drivers::virtio::{R_QUEUE_NOTIFY, VqDesc, reg_r, reg_w};
use core::{ptr, sync::atomic::{fence, Ordering}};
use onyx_core::errno::{Errno, KResult};

/// # Safety
/// Caller provides `length` writable bytes. Syscall context, interrupts off.
pub unsafe fn read(buffer: *mut u8, length: usize) -> KResult<u32> {
    if !present() { return Err(Errno::NoEnt); }
    if length == 0 { return Ok(0); }
    LOCK.lock();
    let result = unsafe { read_locked(buffer, length) };
    LOCK.unlock();
    result
}

unsafe fn read_locked(buffer: *mut u8, length: usize) -> KResult<u32> {
    unsafe {
        reg_w(G_CON.base, 0x64, reg_r(G_CON.base, 0x60)); // Acknowledge polled IRQs.
        if G_CON.rx_offset == G_CON.rx_len {
            let used = ptr::read_volatile(ptr::addr_of!((*G_CON.rx_used).idx));
            if used == G_CON.rx_last { return Err(Errno::Again); }
            fence(Ordering::Acquire);
            let entry = ptr::read_volatile(ptr::addr_of!((*G_CON.rx_used).ring[G_CON.rx_last as usize % QUEUE_SIZE]));
            G_CON.rx_last = G_CON.rx_last.wrapping_add(1);
            G_CON.rx_offset = 0;
            G_CON.rx_len = (entry.len as usize).min(BUF_SIZE);
        }
        let count = length.min(G_CON.rx_len - G_CON.rx_offset);
        ptr::copy_nonoverlapping(G_CON.rx_buf.add(G_CON.rx_offset), buffer, count);
        G_CON.rx_offset += count;
        if G_CON.rx_offset == G_CON.rx_len {
            push(0, true);
            reg_w(G_CON.base, R_QUEUE_NOTIFY, 0);
        }
        if count == 0 { Err(Errno::Again) } else { Ok(count as u32) }
    }
}

/// # Safety
/// Caller provides `length` readable bytes. Syscall context, interrupts off.
pub unsafe fn write(buffer: *const u8, length: usize) -> KResult<u32> {
    if !present() { return Err(Errno::NoEnt); }
    if length == 0 { return Ok(0); }
    LOCK.lock();
    let result = unsafe { write_locked(buffer, length) };
    LOCK.unlock();
    result
}

unsafe fn write_locked(buffer: *const u8, length: usize) -> KResult<u32> {
    unsafe {
        let completed = ptr::read_volatile(ptr::addr_of!((*G_CON.tx_used).idx));
        if G_CON.tx_busy {
            if completed == G_CON.tx_last { return Err(Errno::Again); }
            fence(Ordering::Acquire);
            G_CON.tx_busy = false;
        }
        let count = length.min(BUF_SIZE);
        ptr::copy_nonoverlapping(buffer, G_CON.tx_buf, count);
        *G_CON.tx_desc = VqDesc { addr: G_CON.tx_buf as u64, len: count as u32, flags: 0, next: 0 };
        // Capture completion BEFORE publish/notify: the host may consume the
        // descriptor immediately during the MMIO notify store.
        G_CON.tx_last = completed;
        G_CON.tx_busy = true;
        push(0, false);
        reg_w(G_CON.base, R_QUEUE_NOTIFY, 1);
        reg_w(G_CON.base, 0x64, reg_r(G_CON.base, 0x60));
        Ok(count as u32)
    }
}

pub fn getc() -> Option<u8> {
    let mut byte = 0;
    unsafe { read(&mut byte, 1).ok().map(|_| byte) }
}
pub fn putc(byte: u8) -> KResult<()> { unsafe { write(&byte, 1).map(|_| ()) } }
/// Nonblocking string write; returns the accepted prefix length.
/// The caller must retry the remaining suffix after EAGAIN/partial progress.
pub fn puts(text: &str) -> KResult<u32> { unsafe { write(text.as_ptr(), text.len()) } }
