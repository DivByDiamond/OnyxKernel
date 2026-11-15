#!/usr/bin/env python3
"""Boot legacy redo and full-block undo fixtures through the real kernel."""
import argparse
from pathlib import Path
import selectors
import shutil
import struct
import subprocess
import tempfile
import time

BLOCK = 4096
BASE = 10240 * 512
MAGIC = int.from_bytes(b"ONJU", "little")
ROOT = Path(__file__).resolve().parents[2]


def checksum(data):
    value = 2166136261
    for byte in data:
        value = ((value ^ byte) * 16777619) & 0xFFFFFFFF
    return value


def header(target=None, original=None):
    data = bytearray(BLOCK)
    struct.pack_into("<III", data, 0, MAGIC, 1, int(target is not None))
    if target is not None:
        struct.pack_into("<II", data, 16, target, checksum(original))
    struct.pack_into("<I", data, 508, checksum(data[:508]))
    return data


def run_case(args, mode, directory):
    disk = directory / (mode + ".img")
    shutil.copyfile(args.pristine, disk)
    original = bytes([0x39]) * BLOCK
    changed = bytes([0xA5]) * BLOCK
    with disk.open("r+b") as stream:
        def read(block):
            stream.seek(BASE + block * BLOCK)
            return stream.read(BLOCK)

        def write(block, data):
            stream.seek(BASE + block * BLOCK)
            stream.write(data)

        sb = read(0)
        total, = struct.unpack_from("<I", sb, 12)
        bitmap, start = struct.unpack_from("<II", sb, 24)
        journal, size = struct.unpack_from("<II", sb, 44)
        bits = read(bitmap)
        bit = next(i for i in reversed(range(min(total - start, BLOCK * 8)))
                   if not bits[i // 8] & (1 << (i % 8)))
        target = start + bit
        write(target, original)
        for index in range(size):
            write(journal + index, bytes(BLOCK))
        if mode.startswith("legacy"):
            entry = bytearray(BLOCK)
            struct.pack_into("<II", entry, 0, 1, target)
            entry[8:] = changed[:BLOCK - 8]
            write(journal + 1, entry)
            if mode == "legacy-committed":
                end = bytearray(BLOCK)
                struct.pack_into("<I", end, 0, 2)
                write(journal + 2, end)
            expected = changed[:BLOCK - 8] + original[-8:] if mode == "legacy-committed" else original
        else:
            write(journal + 1, original)
            write(journal, header(target, original) if mode == "undo-pending" else header())
            write(target, changed)
            expected = original if mode == "undo-pending" else changed

    command = ["qemu-system-riscv64", "-machine", "virt", "-m", "256M",
               "-nographic", "-bios", str(args.boot_dir / "bootloader.bin"),
               "-drive", f"file={disk},format=raw,if=none,id=drive0",
               "-device", "virtio-blk-device,drive=drive0"]
    process = subprocess.Popen(command, stdin=subprocess.DEVNULL,
                               stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    output = bytearray()
    selector = selectors.DefaultSelector()
    selector.register(process.stdout, selectors.EVENT_READ)
    try:
        deadline = time.monotonic() + args.timeout
        while time.monotonic() < deadline and b"login:" not in output:
            for key, _ in selector.select(0.1):
                data = key.fileobj.read1(65536)
                if not data:
                    raise RuntimeError(f"{mode}: QEMU exited before login")
                output.extend(data)
        if b"login:" not in output:
            raise RuntimeError(f"{mode}: no login within timeout")
        if b"KERNEL page fault" in output or b"panicked" in output:
            raise RuntimeError(f"{mode}: guest fault")
    finally:
        process.terminate()
        try:
            process.wait(timeout=3)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
        selector.close()
        process.stdout.close()
        (args.logdir / (mode + ".log")).write_bytes(output)
    with disk.open("rb") as stream:
        stream.seek(BASE + target * BLOCK)
        assert stream.read(BLOCK) == expected, f"{mode}: incorrect recovered home block"
        stream.seek(BASE + journal * BLOCK)
        clean = stream.read(BLOCK)
        assert struct.unpack_from("<III", clean) == (MAGIC, 1, 0), f"{mode}: active journal"
        assert struct.unpack_from("<I", clean, 508)[0] == checksum(clean[:508])
    print(f"PASS {mode}: login, expected full home block, clean ONJU header", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pristine", type=Path, default=ROOT / "build/boot.img")
    parser.add_argument("--boot-dir", type=Path, default=ROOT.parent / "OnyxBoot")
    parser.add_argument("--logdir", type=Path, default=Path("/tmp/onyx-journal-e2e"))
    parser.add_argument("--timeout", type=float, default=30)
    args = parser.parse_args()
    args.logdir.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="onyx-journal-") as temporary:
        for mode in ["legacy-committed", "legacy-torn", "undo-pending", "undo-clean"]:
            run_case(args, mode, Path(temporary))
    print("JOURNAL E2E: PASS")


if __name__ == "__main__":
    main()
