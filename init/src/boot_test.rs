#![no_std]
#![no_main]

mod syscalls;

unsafe fn write_str(s: &[u8]) {
    syscalls::write(1, s.as_ptr(), s.len());
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _start() -> ! {
    // ── Test 1: lsblk ──────────────────────────────────────────
    write_str(b"\n=== boot_test: /bin/lsblk ===\n");

    let blk_path = b"/dev/blk0\0";
    let fd = syscalls::open(blk_path.as_ptr(), 0, 0);
    if fd < 0 {
        write_str(b"boot_test: open /dev/blk0 FAILED\n");
    } else {
        let mut mbr = [0u8; 512];
        let n = syscalls::read(fd as u64, mbr.as_mut_ptr(), 512);
        if n == 512 && mbr[510] == 0x55 && mbr[511] == 0xAA {
            write_str(b"  MBR: valid signature\n");
        } else {
            write_str(b"  MBR: no signature (raw fs?)\n");
        }
        syscalls::close(fd as u64);

        // Read OnyxFS superblock (LBA 0 from config, or LBA 0).
        let fd2 = syscalls::open(blk_path.as_ptr(), 0, 0);
        if fd2 >= 0 {
            let mut sb = [0u8; 512];
            let off = 0u64;
            syscalls::lseek(fd2 as u64, off as i64, 0);
            let n2 = syscalls::read(fd2 as u64, sb.as_mut_ptr(), 512);
            if n2 >= 4 && &sb[..4] == b"ONY2" {
                write_str(b"  FS: OnyxFS v2\n");
            } else if n2 >= 4 && &sb[..4] == b"ONY1" {
                write_str(b"  FS: OnyxFS v1\n");
            } else if n2 >= 3 && &sb[..3] == b"FAT" {
                write_str(b"  FS: FAT32\n");
            } else {
                write_str(b"  FS: unknown\n");
            }
            syscalls::close(fd2 as u64);
        }
    }

    // ── Test 2: fb_draw ────────────────────────────────────────
    write_str(b"\n=== boot_test: /dev/fb0 ===\n");

    let fb_path = b"/dev/fb0\0";
    let fb_fd = syscalls::open(fb_path.as_ptr(), 0, 0);
    if fb_fd < 0 {
        write_str(b"boot_test: open /dev/fb0 FAILED\n");
    } else {
        // Get FB info via ioctl
        let mut info = [0u32; 5];
        let r = syscalls::ioctl(fb_fd as u64, 0x4600, info.as_mut_ptr() as u64);
        if r < 0 {
            write_str(b"  ioctl: FAILED (skipping fb test)\n");
        } else {
            let w = info[0];
            let h = info[1];
            let _bpp = info[2];
            let p = info[3];
            let s = info[4];

            // Print info as decimal
            write_str(b"  width=");
            print_dec(w);
            write_str(b" height=");
            print_dec(h);
            write_str(b" pitch=");
            print_dec(p);
            write_str(b" size=");
            print_dec(s);
            write_str(b"\n");

            // mmap the framebuffer and draw a pattern
            let prot = 3;
            let flags = 1;
            let fb = syscalls::mmap(0, s as u64, prot, flags, fb_fd as u64, 0);
            if (fb as i64) < 0 {
                write_str(b"  mmap: FAILED\n");
            } else {
                write_str(b"  mmap OK at vaddr=");
                print_dec(fb as u32);
                write_str(b" size=");
                print_dec(s);
                write_str(b"\n");
                // Test writing one pixel at (0,0)
                let fb_ptr = fb as *mut u8;
                unsafe {
                    *fb_ptr = 0xFF;
                    *(fb_ptr.add(1)) = 0x00;
                    *(fb_ptr.add(2)) = 0x00;
                    *(fb_ptr.add(3)) = 0xFF;
                }
                let val = unsafe { *fb_ptr };
                write_str(b"  first pixel written (val=");
                print_dec(val as u32);
                write_str(b")\n");
            }
        }
        syscalls::close(fb_fd as u64);
    }

    write_str(b"\n=== boot_test: done ===\n");
    syscalls::exit(0);
}

fn print_dec(mut val: u32) {
    let mut buf = [0u8; 11];
    let mut pos = 10;
    if val == 0 {
        unsafe {
            syscalls::write(1, b"0".as_ptr(), 1);
        }
        return;
    }
    while val > 0 && pos > 0 {
        pos -= 1;
        buf[pos] = b'0' + (val % 10) as u8;
        val /= 10;
    }
    unsafe {
        syscalls::write(1, buf.as_ptr().add(pos), 11 - pos);
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
