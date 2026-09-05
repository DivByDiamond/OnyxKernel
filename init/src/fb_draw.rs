#![no_std]
#![no_main]
// TODO(2026-08-27): bin-root allow — raw syscall asm runs inside `unsafe fn`
// wrappers (no_std, per-bin compile); re-evaluate on toolchain/edition bump.
#![allow(unsafe_op_in_unsafe_fn)]

mod syscalls;

const FB_IOCTL_GET_INFO: u64 = 0x4600;

#[repr(C)]
struct FbInfo {
    width: u32,
    height: u32,
    bpp: u32,
    pitch: u32,
    size: u32,
}

#[unsafe(no_mangle)]
/// # Safety
///
/// Process entry point: called directly by the kernel from the ELF entry
/// address; the stack is freshly initialized per the RISC-V calling convention.
pub unsafe extern "C" fn _start() -> ! {
    // Open /dev/fb0
    let fb_path = b"/dev/fb0\0";
    let fd = syscalls::open(fb_path.as_ptr(), 0, 0);
    if fd < 0 {
        syscalls::write(
            1,
            b"fb_draw: open /dev/fb0 failed\n".as_ptr(),
            b"fb_draw: open /dev/fb0 failed\n".len(),
        );
        syscalls::exit(1);
    }

    // Get framebuffer info via ioctl
    let mut info = FbInfo {
        width: 0,
        height: 0,
        bpp: 0,
        pitch: 0,
        size: 0,
    };
    let ret = syscalls::ioctl(fd as u64, FB_IOCTL_GET_INFO, &raw mut info as u64);
    if ret < 0 {
        syscalls::write(
            1,
            b"fb_draw: ioctl failed\n".as_ptr(),
            b"fb_draw: ioctl failed\n".len(),
        );
        syscalls::exit(1);
    }

    // Map framebuffer
    let map_size = info.size as u64;
    let prot = 3;
    let flags = 1;
    let fb = syscalls::mmap(0, map_size, prot, flags, fd as u64, 0);
    if fb < 0 {
        syscalls::write(
            1,
            b"fb_draw: mmap failed\n".as_ptr(),
            b"fb_draw: mmap failed\n".len(),
        );
        syscalls::exit(1);
    }

    syscalls::close(fd as u64);

    // Draw a gradient pattern
    let width = info.width as usize;
    let height = info.height as usize;
    let pitch = info.pitch as usize;
    let fb_ptr = fb as *mut u8;

    for y in 0..height {
        for x in 0..width {
            let off = y * pitch + x * 4;
            let r = ((x * 255) / width) as u8;
            let g = ((y * 255) / height) as u8;
            let b = 128u8;
            *fb_ptr.add(off) = b;
            *fb_ptr.add(off + 1) = g;
            *fb_ptr.add(off + 2) = r;
            *fb_ptr.add(off + 3) = 0xFF;
        }
    }

    syscalls::write(
        1,
        b"fb_draw: gradient drawn (1280x720)\n".as_ptr(),
        b"fb_draw: gradient drawn (1280x720)\n".len(),
    );
    syscalls::exit(0);
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {
        unsafe {
            core::arch::asm!("wfi");
        }
    }
}
