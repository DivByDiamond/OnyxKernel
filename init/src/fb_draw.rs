#![no_std]
#![no_main]

use onyx_init::syscalls;

const FB_IOCTL_GET_INFO: u64 = 0x4600;
#[repr(C)]
struct FbInfo {
    width: u32,
    height: u32,
    bpp: u32,
    pitch: u32,
    size: u32,
}

unsafe fn text(bytes: &[u8]) {
    unsafe {
        syscalls::write(1, bytes.as_ptr(), bytes.len());
    }
}
unsafe fn number(mut value: u32) {
    let mut bytes = [0u8; 10];
    let mut index = bytes.len();
    loop {
        index -= 1;
        bytes[index] = b'0' + (value % 10) as u8;
        value /= 10;
        if value == 0 {
            break;
        }
    }
    unsafe {
        text(&bytes[index..]);
    }
}
unsafe fn fail(message: &[u8]) -> ! {
    unsafe {
        text(message);
        syscalls::exit(1);
    }
}

/// Draws the selected framebuffer, using its actual format/stride, after checking
/// read/write bounds and ioctl validation. Usage: fb_draw [/dev/fbN] [red|green|blue] (default: gradient).
/// # Safety
/// Kernel-provided argc/argv must point to the process's initialized argument array.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn _start(argc: usize, argv: *const *const u8) -> ! {
    unsafe {
        let path = if argc > 1 && !argv.is_null() {
            *argv.add(1)
        } else {
            c"/dev/fb0".as_ptr().cast()
        };
        if path.is_null() {
            fail(b"fb_draw: null path\n");
        }
        let fd = syscalls::open(path, 2, 0);
        if fd < 0 {
            fail(b"fb_draw: open failed\n");
        }
        let mut info = FbInfo {
            width: 0,
            height: 0,
            bpp: 0,
            pitch: 0,
            size: 0,
        };
        if syscalls::ioctl(fd as u64, FB_IOCTL_GET_INFO, &raw mut info as u64) < 0 {
            fail(b"fb_draw: ioctl failed\n");
        }
        if syscalls::ioctl(fd as u64, FB_IOCTL_GET_INFO, 0) >= 0 {
            fail(b"fb_draw: invalid ioctl pointer accepted\n");
        }
        let bytes = (info.bpp / 8) as usize;
        if !matches!(info.bpp, 16 | 32) || info.size == 0 || info.pitch < info.width * bytes as u32
        {
            fail(b"fb_draw: invalid geometry/format\n");
        }
        // Verify the secondary-display devfs read/write paths, including EOF.
        let marker = [0x5au8];
        let mut readback = [0u8];
        if syscalls::lseek(fd as u64, info.size as i64 - 1, 0) < 0
            || syscalls::write(fd as u64, marker.as_ptr(), 1) != 1
            || syscalls::write(fd as u64, marker.as_ptr(), 1) != 0
            || syscalls::lseek(fd as u64, info.size as i64 - 1, 0) < 0
            || syscalls::read(fd as u64, readback.as_mut_ptr(), 1) != 1
            || readback != marker
            || syscalls::read(fd as u64, readback.as_mut_ptr(), 1) != 0
        {
            fail(b"fb_draw: framebuffer read/write bounds failed\n");
        }
        let mapped = syscalls::mmap(0, info.size as u64, 3, 1, fd as u64, 0);
        if mapped < 0 {
            fail(b"fb_draw: mmap failed\n");
        }
        let fb = mapped as *mut u8;
        let color = if argc > 2 && !argv.is_null() {
            let arg = *argv.add(2);
            if arg.is_null() { 0 } else { *arg }
        } else {
            0
        };
        for y in 0..info.height as usize {
            for x in 0..info.width as usize {
                let (red, green, blue) = match color {
                    b'r' => (255, 0, 0),
                    b'g' => (0, 255, 0),
                    b'b' => (0, 0, 255),
                    _ => (
                        (x * 255 / info.width as usize) as u32,
                        (y * 255 / info.height as usize) as u32,
                        128,
                    ),
                };
                let offset = y * info.pitch as usize + x * bytes;
                if info.bpp == 16 {
                    let pixel = ((red >> 3) << 11 | (green >> 2) << 5 | (blue >> 3)) as u16;
                    core::ptr::write_volatile(fb.add(offset).cast::<u16>(), pixel.to_le());
                } else {
                    core::ptr::write_volatile(
                        fb.add(offset).cast::<u32>(),
                        (red << 16 | green << 8 | blue).to_le(),
                    );
                }
            }
        }
        let prefix = mapped as u64 & 0xFFF;
        syscalls::munmap(mapped as u64 - prefix, info.size as u64 + prefix);
        syscalls::close(fd as u64);
        text(b"FB_DRAW_OK ");
        number(info.width);
        text(b"x");
        number(info.height);
        text(b"x");
        number(info.bpp);
        text(b"\n");
        syscalls::exit(0);
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
