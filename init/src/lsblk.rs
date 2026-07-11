#![no_std]
#![no_main]
#![allow(clippy::missing_safety_doc, non_snake_case, unsafe_op_in_unsafe_fn)]

mod syscalls;

#[repr(C)]
#[derive(Clone, Copy)]
struct PartitionEntry {
    status: u8,
    chs_start: [u8; 3],
    type_: u8,
    chs_end: [u8; 3],
    lba_start: u32,
    num_sectors: u32,
}

fn hex_byte(b: u8, out: &mut [u8; 3]) {
    const HEX: &[u8] = b"0123456789ABCDEF";
    out[0] = HEX[(b >> 4) as usize];
    out[1] = HEX[(b & 0xF) as usize];
    out[2] = 0;
}

fn dec_u32(val: u32, out: &mut [u8; 11]) -> usize {
    if val == 0 {
        out[0] = b'0';
        return 1;
    }
    let mut buf = [0u8; 11];
    let mut pos = 10;
    let mut v = val;
    while v > 0 && pos > 0 {
        pos -= 1;
        buf[pos] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    let len = 11 - pos;
    out[..len].copy_from_slice(&buf[pos..]);
    len
}

fn dec_u64(val: u64, out: &mut [u8; 21]) -> usize {
    if val == 0 {
        out[0] = b'0';
        return 1;
    }
    let mut buf = [0u8; 21];
    let mut pos = 20;
    let mut v = val;
    while v > 0 && pos > 0 {
        pos -= 1;
        buf[pos] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    let len = 21 - pos;
    out[..len].copy_from_slice(&buf[pos..]);
    len
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _start() -> ! {
    let blk_path = b"/dev/blk0\0";
    let fd = syscalls::open(blk_path.as_ptr(), 0, 0);
    if fd < 0 {
        syscalls::write(1, b"lsblk: open /dev/blk0 failed\n".as_ptr(), 30);
        syscalls::exit(1);
    }

    // Read MBR (sector 0, 512 bytes)
    let mut mbr = [0u8; 512];
    let n = syscalls::read(fd as u64, mbr.as_mut_ptr(), 512);
    if n != 512 {
        syscalls::write(1, b"lsblk: read failed\n".as_ptr(), 20);
        syscalls::exit(1);
    }

    // Check MBR signature
    if mbr[510] != 0x55 || mbr[511] != 0xAA {
        syscalls::write(1, b"lsblk: no MBR signature\n".as_ptr(), 25);
        syscalls::exit(1);
    }

    syscalls::write(1, b"Device: /dev/blk0\n".as_ptr(), 19);
    syscalls::write(1, b"MBR signature: OK\n".as_ptr(), 20);

    // Parse partition table (offset 446, 4 entries)
    let parts = &mbr[446..510];
    let entries: *const PartitionEntry = parts.as_ptr() as *const PartitionEntry;
    let mut found = false;

    for i in 0..4 {
        let e = &*entries.add(i);
        if e.type_ == 0 {
            continue;
        }
        if !found {
            syscalls::write(1, b"\nPartitions:\n".as_ptr(), 13);
            found = true;
        }

        // "  #N: type=0xXX LBA=XXXXXXXX sectors=XXXXXXXX size=XXX MB\n"
        let mut line = [0u8; 80];
        let mut pos = 0;
        line[pos] = b' ';
        pos += 1;
        line[pos] = b' ';
        pos += 1;
        line[pos] = b'#';
        pos += 1;
        line[pos] = b'1' + i as u8;
        pos += 1;
        line[pos] = b':';
        pos += 1;
        line[pos] = b' ';
        pos += 1;

        let type_label = b"type=0x";
        line[pos..pos + 7].copy_from_slice(type_label);
        pos += 7;
        let mut hex_buf = [0u8; 3];
        hex_byte(e.type_, &mut hex_buf);
        line[pos..pos + 2].copy_from_slice(&hex_buf[..2]);
        pos += 2;
        line[pos] = b' ';
        pos += 1;

        let lba_label = b"LBA=";
        line[pos..pos + 4].copy_from_slice(lba_label);
        pos += 4;
        let mut dec_buf = [0u8; 11];
        let nd = dec_u32(e.lba_start, &mut dec_buf);
        line[pos..pos + nd].copy_from_slice(&dec_buf[..nd]);
        pos += nd;
        line[pos] = b' ';
        pos += 1;

        let sec_label = b"sectors=";
        line[pos..pos + 8].copy_from_slice(sec_label);
        pos += 8;
        let ns = dec_u32(e.num_sectors, &mut dec_buf);
        line[pos..pos + ns].copy_from_slice(&dec_buf[..ns]);
        pos += ns;
        line[pos] = b' ';
        pos += 1;

        let size_sectors = e.num_sectors as u64;
        let size_bytes = size_sectors * 512;
        let size_mb = size_bytes / (1024 * 1024);
        let mb_label = b"size=";
        line[pos..pos + 5].copy_from_slice(mb_label);
        pos += 5;
        let mut mb_buf = [0u8; 21];
        let nm = dec_u64(size_mb, &mut mb_buf);
        line[pos..pos + nm].copy_from_slice(&mb_buf[..nm]);
        pos += nm;
        let mb_suffix = b" MB\n";
        line[pos..pos + 4].copy_from_slice(mb_suffix);
        pos += 4;

        syscalls::write(1, line.as_ptr(), pos);
    }

    if !found {
        syscalls::write(1, b"  (no partitions found)\n".as_ptr(), 24);
    }

    syscalls::close(fd as u64);
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
