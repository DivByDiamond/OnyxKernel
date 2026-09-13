#!/bin/bash
set -e
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"
BOOT_DIR="${ONYXBOOT_DIR:-$ROOT/../OnyxBoot}"
BUILD="$ROOT/build"

"$HERE/build_image.sh"

echo "==> Starting QEMU"
QEMU_DISPLAY="${QEMU_DISPLAY:-none}"
qemu-system-riscv64 \
    -M virt -m 256M -smp 2 \
    -bios "$BOOT_DIR/bootloader.bin" \
    -drive file="$BUILD/boot.img",format=raw,if=none,id=drive0 \
    -device virtio-blk-device,drive=drive0 \
    -serial stdio \
    -display "$QEMU_DISPLAY" -no-reboot
