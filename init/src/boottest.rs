use crate::syscalls;
use crate::util::write_dec;

pub(crate) unsafe fn devfs_boot_test() {
    let m = b"[init] devfs boot test start\n";
    syscalls::write(1, m.as_ptr(), m.len());

    let blk_path = b"/dev/blk0\0";
    let fd = syscalls::open(blk_path.as_ptr(), 0, 0);
    if fd < 0 {
        let m = b"[init]   /dev/blk0: open FAILED\n";
        syscalls::write(1, m.as_ptr(), m.len());
    } else {
        let mut mbr = [0u8; 512];
        let n = syscalls::read(fd as u64, mbr.as_mut_ptr(), 512);
        if n == 512 && mbr[510] == 0x55 && mbr[511] == 0xAA {
            let m = b"[init]   /dev/blk0: MBR valid, FS signature reading...\n";
            syscalls::write(1, m.as_ptr(), m.len());
            syscalls::lseek(fd as u64, 10240i64 * 512, 0);
            let mut sb = [0u8; 512];
            let n2 = syscalls::read(fd as u64, sb.as_mut_ptr(), 512);
            if n2 >= 4 {
                if &sb[..4] == b"ONY2" {
                    let m = b"[init]   blk0: OnyxFS v2 OK\n";
                    syscalls::write(1, m.as_ptr(), m.len());
                } else {
                    let m = b"[init]   blk0: FS unknown (not OnyxFS)\n";
                    syscalls::write(1, m.as_ptr(), m.len());
                }
            }
        } else {
            let m = b"[init]   /dev/blk0: MBR signature NOT found\n";
            syscalls::write(1, m.as_ptr(), m.len());
        }
        syscalls::close(fd as u64);
    }

    let fb_path = b"/dev/fb0\0";
    let fb_fd = syscalls::open(fb_path.as_ptr(), 0, 0);
    if fb_fd < 0 {
        let m = b"[init]   /dev/fb0: open FAILED\n";
        syscalls::write(1, m.as_ptr(), m.len());
    } else {
        let mut info = [0u32; 5];
        let r = syscalls::ioctl(fb_fd as u64, 0x4600, info.as_mut_ptr() as u64);
        if r < 0 {
            let m = b"[init]   /dev/fb0: ioctl FAILED\n";
            syscalls::write(1, m.as_ptr(), m.len());
        } else {
            let m = b"[init]   /dev/fb0: ";
            syscalls::write(1, m.as_ptr(), m.len());
            syscalls::write(1, b"w=".as_ptr(), 2);
            write_dec(info[0] as i64);
            syscalls::write(1, b" h=".as_ptr(), 3);
            write_dec(info[1] as i64);
            syscalls::write(1, b" bpp=".as_ptr(), 5);
            write_dec(info[2] as i64);
            syscalls::write(1, b" pitch=".as_ptr(), 7);
            write_dec(info[3] as i64);
            syscalls::write(1, b" size=".as_ptr(), 6);
            write_dec(info[4] as i64);
            syscalls::write(1, b"\n".as_ptr(), 1);
        }
        syscalls::close(fb_fd as u64);
    }

    let m = b"[init] devfs boot test done\n";
    syscalls::write(1, m.as_ptr(), m.len());
}
