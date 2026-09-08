#!/usr/bin/env python3
"""Journal crash-recovery e2e test for OnyxFS v2 (todo.md: "[~] journal
crash-recovery с реальным блочным I/O (ручной QEMU-цикл)").

Automated QEMU loop:
 1. Take pristine boot.img (FAT32 @ LBA 2048 + OnyxFS @ LBA 10240).
 2. Parse the OnyxFS superblock (block 0 of the partition, 4096-byte FS blocks).
 3. Inject a synthetic COMMITTED journal transaction: two block_write entries
    + commit_end, writing a payload to a free data block. The data block
    itself is left with OLD content (crash before data write).
 4. Boot QEMU: mount() runs journal_recover() -> must replay payload into
    the data block, zero the journal, and boot cleanly.
 5. After QEMU exits, re-read the image: PASS if data block now contains the
    payload and journal area is zeroed.
 6. Negative control: inject WITHOUT commit_end (torn transaction) ->
    recovery must discard: data block unchanged, journal zeroed anyway.

Writes nothing to the repo; works on copies in /tmp/onyx_stress.
"""
import struct, subprocess, time, os, shutil, sys

PRISTINE = "/tmp/onyx_stress/pristine.img"
LOGDIR = "/tmp/onyx_stress"
BOOT_DIR = "/storage/project/systems/Onyx/OnyxBoot"
ONYXFS_LBA = 10240
BS = 4096

def read_block(img, blk):
    with open(img, "rb") as f:
        f.seek(ONYXFS_LBA * 512 + blk * BS)
        return f.read(BS)

def write_block(img, blk, data):
    with open(img, "r+b") as f:
        f.seek(ONYXFS_LBA * 512 + blk * BS)
        f.write(data)

def parse_sb(img):
    sb = read_block(img, 0)
    magic, version, block_size, total_blocks, inode_count, \
        inode_table_start, data_bitmap_start, data_blocks_start, root_inode, \
        snapshot_area_start, snapshot_count, journal_start, journal_size, \
        feature_flags = struct.unpack_from("<14I", sb, 0)
    assert magic == 0x32594E4F, hex(magic)
    return dict(total_blocks=total_blocks, inode_table_start=inode_table_start,
                data_bitmap_start=data_bitmap_start,
                data_blocks_start=data_blocks_start,
                journal_start=journal_start, journal_size=journal_size,
                feature_flags=feature_flags)

def inject(img, sb, with_commit):
    js, jsize = sb["journal_start"], sb["journal_size"]
    assert jsize >= 5, f"journal too small: {jsize}"
    # target: a data block near the end of the image, unlikely to hold
    # live data at boot; we verify recovery purely by content match.
    target_blk = sb["total_blocks"] - 2
    payload = b"ONYX-JRNL-RECOVER-E2E-" + b"\xA5" * (300 - 22)
    e1 = bytearray(BS); e2 = bytearray(BS); e3 = bytearray(BS)
    struct.pack_into("<II", e1, 0, 0, target_blk)        # commit_start
    struct.pack_into("<II", e2, 0, 1, target_blk)        # block_write
    e2[8:8+len(payload)] = payload
    struct.pack_into("<II", e3, 0, 2, 0)                 # commit_end
    write_block(img, js + 0, bytes(e1))
    write_block(img, js + 1, bytes(e2))
    if with_commit:
        write_block(img, js + 2, bytes(e3))
    # ensure target block starts with OLD (zero) content
    old = read_block(img, target_blk)
    assert old[:len(payload)] != payload
    return target_blk, payload

def boot_and_wait(disk, tag, wait_s=55):
    log = open(f"{LOGDIR}/jrnl_{tag}.log", "wb")
    p = subprocess.Popen(
        ["qemu-system-riscv64", "-machine", "virt", "-m", "256M", "-smp", "1",
         "-nographic", "-no-reboot", "-bios", f"{BOOT_DIR}/bootloader.bin",
         "-drive", f"file={disk},format=raw,if=none,id=drive0",
         "-device", "virtio-blk-device,drive=drive0"],
        stdout=log, stderr=log, stdin=subprocess.PIPE, preexec_fn=os.setsid)
    time.sleep(wait_s)
    p.kill(); log.close()
    with open(f"{LOGDIR}/jrnl_{tag}.log", errors="replace") as f:
        return f.read()

def journal_zeroed(img, sb):
    for j in range(min(sb["journal_size"], 6)):
        if any(read_block(img, sb["journal_start"] + j)[:64]):
            return False
    return True

def main():
    sb_copy = f"{LOGDIR}/jrnl_sb.img"
    shutil.copyfile(PRISTINE, sb_copy)
    sb = parse_sb(sb_copy)
    print(f"SB: total={sb['total_blocks']} data_start={sb['data_blocks_start']} "
          f"journal_start={sb['journal_start']} journal_size={sb['journal_size']} "
          f"flags={hex(sb['feature_flags'])}")
    ok = True

    # --- positive: committed transaction must replay ---
    pos = f"{LOGDIR}/jrnl_pos.img"
    shutil.copyfile(PRISTINE, pos)
    target, payload = inject(pos, sb, with_commit=True)
    print(f"[+] injected COMMITTED tx -> block {target}")
    log = boot_and_wait(pos, "pos")
    data = read_block(pos, target)
    replayed = data[:len(payload)] == payload
    print(f"[+] replayed: {replayed}; journal zeroed after mount: {journal_zeroed(pos, sb)}")
    print(f"[+] boot log: mount_ok={'OnyxFS mounted' in log or 'onyxfs' in log.lower()}, "
          f"panic={'panicked' in log}")
    ok &= replayed and journal_zeroed(pos, sb)

    # --- negative: torn transaction (no commit) must be discarded ---
    neg = f"{LOGDIR}/jrnl_neg.img"
    shutil.copyfile(PRISTINE, neg)
    target2, payload2 = inject(neg, sb, with_commit=False)
    print(f"[-] injected TORN tx -> block {target2}")
    log2 = boot_and_wait(neg, "neg")
    data2 = read_block(neg, target2)
    discarded = data2[:len(payload2)] != payload2
    print(f"[-] discarded (block unchanged): {discarded}; "
          f"journal zeroed after mount: {journal_zeroed(neg, sb)}")
    print(f"[-] boot log: panic={'panicked' in log2}")
    ok &= discarded and journal_zeroed(neg, sb)

    print("\n=== JOURNAL E2E:", "PASS" if ok else "FAIL", "===")
    return 0 if ok else 1

if __name__ == "__main__":
    sys.exit(main())
