//! TUI Demo — real framebuffer + event loop (todo P3 #5).
//!
//! Follows the fb_draw pattern (open /dev/fb0 -> FB_IOCTL_GET_INFO -> mmap)
//! and replaces the null fb pointer with the mapped framebuffer. The event
//! loop uses poll(stdin, 50 ms) (todo P1 #1) so the demo redraws on a
//! timer-free tick and reacts to keys; ESC exits. Ctrl+Z now stops the
//! process instead of killing it (todo P2), and the kernel mouse snapshot
//! (todo P3 #1) is polled via SYS_mouse_read when available.

#![no_std]
#![no_main]
#![allow(unsafe_op_in_unsafe_fn)]

mod libtui;
mod syscalls;

use libtui::{Button, Event, Label, Layout, TextBox, Widget, font};

const FB_IOCTL_GET_INFO: u64 = 0x4600;
const POLLIN: i32 = 0x001;

const WIDTH: usize = 1280;
const HEIGHT: usize = 720;

#[repr(C)]
struct FbInfo {
    width: u32,
    height: u32,
    bpp: u32,
    pitch: u32,
    size: u32,
}

#[repr(C)]
struct MouseEvt {
    x: u16,
    y: u16,
    buttons: u8,
    _pad: u8,
}

/// pollfd record matching the kernel ABI (16 bytes, no padding).
#[repr(C)]
struct PollFd {
    fd: i64,
    events: i32,
    revents: i32,
}

#[unsafe(no_mangle)]
/// # Safety
///
/// Process entry point: called by the kernel with a fresh user stack.
pub unsafe extern "C" fn _start() -> ! {
    // Map the framebuffer (fb_draw pattern: open -> ioctl -> mmap).
    let fb_path = b"/dev/fb0\0";
    let fd = syscalls::open(fb_path.as_ptr(), 0, 0);
    if fd < 0 {
        syscalls::write(1, b"tui_demo: open /dev/fb0 failed\n".as_ptr(), 31);
        syscalls::exit(1);
    }
    let mut info = FbInfo {
        width: 0,
        height: 0,
        bpp: 0,
        pitch: 0,
        size: 0,
    };
    if syscalls::ioctl(fd as u64, FB_IOCTL_GET_INFO, &raw mut info as u64) < 0 {
        syscalls::write(1, b"tui_demo: ioctl failed\n".as_ptr(), 23);
        syscalls::exit(1);
    }
    let fb_ptr = syscalls::mmap(0, info.size as u64, 3, 1, fd as u64, 0);
    if fb_ptr <= 0 {
        syscalls::write(1, b"tui_demo: mmap failed\n".as_ptr(), 22);
        syscalls::exit(1);
    }
    syscalls::close(fd as u64);

    // Load the PSF font for widget text rendering.
    font::init();

    let stride = (info.pitch / 4) as usize;
    let pixels = (info.size as usize) / 4;
    let fb = core::slice::from_raw_parts_mut(fb_ptr as *mut u32, pixels);

    // Widget set (static texts; TextBox is the only stateful widget).
    let mut layout = Layout::new(50, 50);
    let label = Label {
        text: "OnyxOS TUI Demo",
        x: layout.x,
        y: layout.y,
    };
    layout.next(20);
    let mut button1 = Button {
        text: "Button 1",
        width: 120,
        height: 40,
        x: layout.x,
        y: layout.y,
        color: 0x4CAF50,
    };
    layout.next(40);
    let mut button2 = Button {
        text: "Exit",
        width: 120,
        height: 40,
        x: layout.x,
        y: layout.y,
        color: 0xF44336,
    };
    layout.next(40);
    let mut textbox = TextBox {
        buffer: [0; 64],
        cursor: 0,
        len: 0,
        x: layout.x,
        y: layout.y,
        width: 200,
    };

    // Event loop: poll stdin with a 50 ms timeout, redraw each pass.
    let mut pfd = PollFd {
        fd: 0,
        events: POLLIN,
        revents: 0,
    };
    let mut mouse = MouseEvt {
        x: 0,
        y: 0,
        buttons: 0,
        _pad: 0,
    };
    loop {
        clear(fb, 0x101418, stride, WIDTH, HEIGHT);
        label.draw(fb, label.x, label.y, stride);
        button1.draw(fb, button1.x, button1.y, stride);
        button2.draw(fb, button2.x, button2.y, stride);
        // bounds() fit check: draw the textbox only when fully visible.
        let (tw, th) = textbox.bounds();
        if textbox.x + tw <= stride as u16 && textbox.y + th <= HEIGHT as u16 {
            textbox.draw(fb, textbox.x, textbox.y, stride);
        }

        let ready = syscalls::poll(&raw mut pfd as *mut u8, 1, 50);
        if ready > 0 && pfd.revents & POLLIN != 0 {
            let mut b: [u8; 1] = [0];
            let n = syscalls::read(0, b.as_mut_ptr(), 1);
            if n == 1 {
                if b[0] == 27 {
                    // ESC exits the demo.
                    syscalls::exit(0);
                }
                textbox.handle_event(&Event::KeyPress(b[0]));
            }
        }
        // Pointer snapshot (kernel accumulates virtio-input mouse events):
        // moves/clicks flow through the widget set; Exit ends the demo.
        if syscalls::mouse_read(&raw mut mouse as *mut MouseEvt as *mut u8) == 0 {
            let move_ev = Event::MouseMove {
                x: mouse.x,
                y: mouse.y,
            };
            let _ = textbox.handle_event(&move_ev);
            if mouse.buttons & 1 != 0 {
                let click = Event::MouseClick {
                    x: mouse.x,
                    y: mouse.y,
                    button: 1,
                };
                if button2.handle_event(&click) {
                    syscalls::exit(0);
                }
                let _ = button1.handle_event(&click);
            }
        } else {
            // No pointer report this tick: deliver the explicit no-event so
            // widgets observe a steady heartbeat.
            let _ = textbox.handle_event(&Event::None);
        }
    }
}

/// Fill the visible screen area with a solid color.
fn clear(fb: &mut [u32], color: u32, stride: usize, w: usize, h: usize) {
    for y in 0..h {
        for x in 0..w {
            let idx = y * stride + x;
            if idx < fb.len() {
                fb[idx] = color;
            }
        }
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {
        unsafe {
            core::arch::asm!("wfi");
        }
    }
}
