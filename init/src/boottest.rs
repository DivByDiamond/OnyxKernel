use crate::syscalls;
use crate::util::write_dec;

const BLK_DEV_MAX: u64 = 8;
const EMBED_LBA: i64 = 10240;

unsafe fn print_blk(idx: u64, msg: &[u8]) {
    syscalls::write(1, b"[init] /dev/blk".as_ptr(), 15);
    write_dec(idx as i64);
    syscalls::write(1, b": ".as_ptr(), 2);
    syscalls::write(1, msg.as_ptr(), msg.len());
}

unsafe fn probe_blk(idx: u64) -> bool {
    let mut path = [0u8; 16];
    path[..8].copy_from_slice(b"/dev/blk");
    path[8] = b'0' + idx as u8;
    let fd = syscalls::open(path.as_ptr(), 0, 0);
    if fd < 0 {
        print_blk(idx, b"open FAILED\n");
        return false;
    }

    let mut sec = [0u8; 512];
    let n = syscalls::read(fd as u64, sec.as_mut_ptr(), 512);
    if n == 512 && &sec[..4] == b"ONY2" {
        print_blk(idx, b"OnyxFS v2 OK\n");
    } else if n == 512 && sec[510] == 0x55 && sec[511] == 0xAA {
        syscalls::lseek(fd as u64, EMBED_LBA * 512, 0);
        let mut sb = [0u8; 512];
        let n2 = syscalls::read(fd as u64, sb.as_mut_ptr(), 512);
        if n2 >= 4 && &sb[..4] == b"ONY2" {
            print_blk(idx, b"OnyxFS v2 OK\n");
        } else {
            print_blk(idx, b"no FS\n");
        }
    } else {
        print_blk(idx, b"no FS\n");
    }

    syscalls::close(fd as u64);
    true
}

pub(crate) unsafe fn devfs_boot_test() {
    let m = b"[init] devfs boot test start\n";
    syscalls::write(1, m.as_ptr(), m.len());

    for idx in 0..BLK_DEV_MAX {
        if !probe_blk(idx) {
            break;
        }
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
