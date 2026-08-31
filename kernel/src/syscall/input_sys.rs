//! Input device syscalls (mouse, keyboard events)

use crate::syscall::handler::user_ptr_ok;
use onyx_core::errno::Errno;

/// Mouse event structure (matches userspace ABI)
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MouseEvent {
    pub x: u16,
    pub y: u16,
    pub buttons: u8, // bit 0=left, 1=right, 2=middle
}

/// SYS_mouse_read - read mouse event
///
/// # Safety
///
/// `event_ptr` must be a valid user pointer to MouseEvent struct.
pub unsafe fn sys_mouse_read(event_ptr: *mut MouseEvent) -> i64 {
    // Validate user pointer
    if !user_ptr_ok(event_ptr as u64, core::mem::size_of::<MouseEvent>() as u64) {
        return Errno::Fault.as_i64();
    }

    // TODO: Read from virtio-input driver
    // For MVP: return stub data (no mouse input yet)
    unsafe {
        (*event_ptr).x = 0;
        (*event_ptr).y = 0;
        (*event_ptr).buttons = 0;
    }

    0 // success
}
