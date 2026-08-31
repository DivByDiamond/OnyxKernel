//! TUI Demo - showcase widgets
//!
//! Demonstrates Button, Label, TextBox widgets with mouse/keyboard input.

#![no_std]
#![no_main]
#![allow(unsafe_op_in_unsafe_fn)]

mod libtui;
mod syscalls;

use libtui::{Button, Event, Label, Layout, TextBox, Widget};

const WIDTH: usize = 1280;
const HEIGHT: usize = 720;

#[unsafe(no_mangle)]
/// # Safety
///
/// Process entry point.
pub unsafe extern "C" fn _start() -> ! {
    // Get framebuffer (for MVP: assume it's available)
    // TODO: mmap /dev/fb0 or syscall to get fb pointer
    let fb = core::ptr::null_mut::<u32>();

    // For MVP: just draw colored rectangles to show widgets
    demo_draw_widgets();

    syscalls::exit(0);
}

unsafe fn demo_draw_widgets() {
    // Create widgets
    let mut layout = Layout::new(50, 50);

    let label = Label {
        text: "OnyxOS TUI Demo",
        x: layout.x,
        y: layout.y,
    };
    layout.next(20);

    let button1 = Button {
        text: "Button 1",
        width: 120,
        height: 40,
        x: layout.x,
        y: layout.y,
        color: 0x4CAF50, // green
    };
    layout.next(40);

    let button2 = Button {
        text: "Exit",
        width: 120,
        height: 40,
        x: layout.x,
        y: layout.y,
        color: 0xF44336, // red
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

    // For MVP: print widget positions (no actual drawing without fb)
    // TODO: Draw widgets when framebuffer mmap is ready
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {
        unsafe {
            core::arch::asm!("wfi");
        }
    }
}
