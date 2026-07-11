#!/bin/bash
set -e
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"
BOOT_DIR="${ONYXBOOT_DIR:-$ROOT/../OnyxBoot}"

# Build OnyxBoot bootloader
echo "==> Building OnyxBoot"
make -C "$BOOT_DIR" CROSS=riscv64-elf clean all 2>&1 | tail -3

# Build OnyxKernel + init + tools
echo "==> Building OnyxKernel"
cd "$ROOT"
cargo build --release -p onyx_kernel --target riscv64gc-unknown-none-elf 2>&1 | tail -3
cargo build --release -p onyx_init --target riscv64gc-unknown-none-elf 2>&1 | tail -3
cargo build --release -p onyx_tools 2>&1 | tail -3

# Build OnyxShell (separate project outside workspace)
SHELL_DIR="${ONYXSHELL_DIR:-$ROOT/../OnyxShell}"
echo "==> Building OnyxShell"
"$SHELL_DIR/build.sh"

# Convert all userland ELFs to .onx (v2 format is now the default)
BUILD="$ROOT/build"
mkdir -p "$BUILD"
echo "==> Converting userland ELFs → .onx (v2 default, --compress)"
"$ROOT/target/release/elf2onx" --ring=1 --compress "$ROOT/target/riscv64gc-unknown-none-elf/release/onyx-init" "$BUILD/init.onx"
"$ROOT/target/release/elf2onx" --ring=1 --compress "$ROOT/target/riscv64gc-unknown-none-elf/release/onyx-hello" "$BUILD/hello.onx"
"$ROOT/target/release/elf2onx" --ring=1 --compress "$ROOT/target/riscv64gc-unknown-none-elf/release/onyx-login" "$BUILD/login.onx"
cp "$SHELL_DIR/build/osh.onx" "$BUILD/osh.onx"
"$ROOT/target/release/elf2onx" --compress "$ROOT/target/riscv64gc-unknown-none-elf/release/onyx-passwd" "$BUILD/passwd.onx"
"$ROOT/target/release/elf2onx" --ring=1 --compress "$ROOT/target/riscv64gc-unknown-none-elf/release/onyx-useradd" "$BUILD/useradd.onx"
"$ROOT/target/release/elf2onx" --ring=1 --compress "$ROOT/target/riscv64gc-unknown-none-elf/release/onyx-userdel" "$BUILD/userdel.onx"
"$ROOT/target/release/elf2onx" --compress "$ROOT/target/riscv64gc-unknown-none-elf/release/onyx-argv-test" "$BUILD/argv_test.onx"
"$ROOT/target/release/elf2onx" --compress "$ROOT/target/riscv64gc-unknown-none-elf/release/onyx-fb-draw" "$BUILD/fb_draw.onx"
"$ROOT/target/release/elf2onx" --ring=1 --compress "$ROOT/target/riscv64gc-unknown-none-elf/release/onyx-lsblk" "$BUILD/lsblk.onx"

# NOTE: /etc/passwd and /etc/shadow are NOT pre-baked into the image.
# On first boot, /bin/login detects that no root user exists and calls
# ensure_default_root() which creates both files with the default
# password "root". This guarantees the hash algorithm matches between
# creation and verification.

# Build OnyxCC .onx
echo "==> Building OnyxCC"
ONYXCCDIR="$ROOT/../OnyxCompiller"
if [ -f "$ONYXCCDIR/onyxcc.onx" ]; then
    cp "$ONYXCCDIR/onyxcc.onx" "$BUILD/onyxcc.onx"
else
    echo "onyxcc.onx not found — skipping"
fi

# Generate PSF1 font
echo "==> Generating font"
"$ROOT/target/release/psfgen" "$BUILD/default.psf"

# Create enable-flag for the lsblk boot-time service.
echo "1" > "$BUILD/lsblk.enabled" 2>/dev/null || true

# Create manifest. Optional files (onyxcc, test.c) are added only if
# they exist — otherwise mkimage would fail trying to read them.
MANIFEST="$BUILD/manifest.txt"
{
    echo "dir /bin"
    echo "dir /etc"
    echo "dir /etc/init"
    echo "dir /service"
    echo "dir /users"
    echo "dir /font"
    echo "file $BUILD/hello.onx /bin/hello.onx --ring=1"
    echo "file $BUILD/init.onx /bin/init --ring=1"
    echo "file $BUILD/login.onx /bin/login --ring=1"
    echo "file $BUILD/osh.onx /bin/osh"
    echo "file $BUILD/passwd.onx /bin/passwd"
    echo "file $BUILD/useradd.onx /bin/useradd --ring=1"
    echo "file $BUILD/userdel.onx /bin/userdel --ring=1"
    echo "file $BUILD/default.psf /font/default.psf"
    if [ -f "$BUILD/onyxcc.onx" ]; then
        echo "file $BUILD/onyxcc.onx /bin/onyxcc --ring=1"
    fi
    echo "file $BUILD/argv_test.onx /bin/argv_test"
    echo "file $BUILD/fb_draw.onx /bin/fb_draw --ring=1"
    echo "file $BUILD/lsblk.onx /bin/lsblk --ring=1"
    echo "file $BUILD/lsblk.onx /service/lsblk --ring=1"
    echo "file $BUILD/lsblk.enabled /etc/init/lsblk.enabled"
    ONYXCC_TEST_C="$ROOT/../OnyxCompiller/tests/hello_full.c"
    if [ -f "$ONYXCC_TEST_C" ]; then
        echo "file $ONYXCC_TEST_C /tmp/test.c"
    fi
} > "$MANIFEST"

# Create OnyxFS v2 disk image using manifest (v2 is now the default)
echo "==> Creating OnyxFS v2 disk image"
"$ROOT/target/release/mkimage" "$BUILD/manifest.txt" "$BUILD/disk.img"

# Create partitioned boot disk
echo "==> Creating partitioned boot disk"
FAT_LBA=2048
dd if=/dev/zero of="$BUILD/boot.img" bs=1M count=64 2>/dev/null
parted -s "$BUILD/boot.img" mklabel msdos 2>/dev/null
parted -s "$BUILD/boot.img" mkpart primary fat32 1MiB 5MiB 2>/dev/null
mkfs.fat -F 32 "$BUILD/boot.img" --offset=$FAT_LBA 2>/dev/null
mcopy -i "$BUILD/boot.img@@$((FAT_LBA * 512))" "$ROOT/target/riscv64gc-unknown-none-elf/release/onyx-kernel" ::kernel.elf 2>/dev/null
SLBA=10240
dd if="$BUILD/disk.img" of="$BUILD/boot.img" bs=512 seek=$SLBA conv=notrunc 2>/dev/null

echo "==> Starting QEMU"
QEMU_DISPLAY="${QEMU_DISPLAY:-none}"
qemu-system-riscv64 \
    -M virt -m 256M -smp 2 \
    -bios "$BOOT_DIR/bootloader.bin" \
    -drive file="$BUILD/boot.img",format=raw,if=none,id=drive0 \
    -device virtio-blk-device,drive=drive0 \
    -serial stdio \
    -display "$QEMU_DISPLAY" -no-reboot
