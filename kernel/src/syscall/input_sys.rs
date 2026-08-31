//! Input device syscalls (mouse, keyboard events).
//!
//! SYS_mouse_read (todo P3 #1): returns the current cursor snapshot from
//! the kernel mouse model (drivers/input/mouse.rs). The virtio-input queue
//! is pumped first (same pattern as sys_net_recv calling net::poll) so a
//! poll-free mouse_read still reports fresh state.

use crate::drivers::input;
use crate::syscall::handler::user_ptr_ok;
use crate::{mm::vmm, proc};
use onyx_core::errno::Errno;

/// Mouse event structure (matches userspace ABI): 8 bytes, packed as
/// {u16 x, u16 y, u8 buttons, u8 pad, u16 pad}.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MouseEvent {
    pub x: u16,
    pub y: u16,
    /// bit 0 = left, 1 = right, 2 = middle.
    pub buttons: u8,
    pub _pad: u8,
}

/// SYS_mouse_read — read the pointer snapshot.
///
/// # Safety
///
/// `event_ptr` must be a valid user pointer to an 8-byte MouseEvent; it is
/// validated here (range + per-page writable mapping) before any write.
pub unsafe fn sys_mouse_read(event_ptr: *mut MouseEvent) -> i64 {
    // SAFETY: event_ptr passed user_ptr_ok and the per-page writable
    // check_user_range below, so the direct field writes only touch a
    // mapped user page; poll_all dispatches kernel-owned input state.
    unsafe {
        let len = core::mem::size_of::<MouseEvent>() as u64;
        if !user_ptr_ok(event_ptr as u64, len)
            || vmm::check_user_range(proc::current().root_pa, event_ptr as u64, len, true).is_err()
        {
            return Errno::Fault.as_i64();
        }
        // Pump the input queue so the snapshot is fresh (virtio-input).
        input::poll_all();
        let (x, y, buttons) = input::mouse::snapshot();
        (*event_ptr).x = x as u16;
        (*event_ptr).y = y as u16;
        (*event_ptr).buttons = buttons;
        (*event_ptr)._pad = 0;
        0
    }
}
