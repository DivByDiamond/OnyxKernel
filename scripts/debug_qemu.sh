#!/bin/bash
# Ready-to-use live-debug launcher for OnyxKernel.
#
# Boots the kernel directly under QEMU (-bios default -kernel, no OnyxBoot
# round trip) and attaches GDB with the breakpoint set from debug.gdb,
# targeting the scheduler/trap functions implicated in the open SMP crash
# (see todo.md). Uses the `debugdev` cargo profile: opt-level=1 for
# tolerable QEMU emulation speed, DWARF kept for GDB (a plain debug build
# is >10x slower to emulate and impractical; release strips debuginfo).
#
# Usage:
#   scripts/debug_qemu.sh            # build + boot paused under GDB (default)
#   scripts/debug_qemu.sh run        # build + normal interactive boot, no GDB
#   scripts/debug_qemu.sh attach     # GDB-connect to a QEMU already running
#                                    # with `-s` (started by hand or elsewhere)
#
# Environment:
#   PROFILE=debugdev|release   cargo profile to build (default: debugdev)
#   SMP=1..8                   guest harts (default: 2; MAX_HARTS = 8)
#   MEM=256M                   guest RAM (default: 256M)
#   GDBPORT=1234               GDB stub port (default: 1234)
#
# The kernel loads /bin/init from the existing build/boot.img disk, so run
# scripts/run_qemu.sh once first if no disk image exists yet.
set -e

HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"
MODE="${1:-gdb}"
PROFILE="${PROFILE:-debugdev}"
SMP="${SMP:-2}"
MEM="${MEM:-256M}"
GDBPORT="${GDBPORT:-1234}"
TARGET_DIR="$ROOT/target/riscv64gc-unknown-none-elf"
ELF="$TARGET_DIR/$PROFILE/onyx-kernel"
LOG="$ROOT/build/debug-qemu.log"

mkdir -p "$ROOT/build"

if [[ "$MODE" != "attach" ]]; then
    echo "==> Building kernel (profile: $PROFILE)"
    (cd "$ROOT" && cargo build --profile "$PROFILE" -p onyx_kernel \
        --target riscv64gc-unknown-none-elf)
fi

QEMU_ARGS=(
    -M virt -m "$MEM" -smp "$SMP"
    -bios default
    -kernel "$ELF"
    -drive file="$ROOT/build/boot.img",format=raw,if=none,id=drive0
    -device virtio-blk-device,drive=drive0
    -display none -no-reboot
)

case "$MODE" in
gdb)
    echo "==> Starting QEMU (paused, GDB stub on :$GDBPORT, serial log: $LOG)"
    rm -f "$LOG"
    qemu-system-riscv64 "${QEMU_ARGS[@]}" -s -S -serial "file:$LOG" &
    QEMU_PID=$!
    trap 'kill $QEMU_PID 2>/dev/null || true' EXIT
    gdb -q \
        -ex "file $ELF" \
        -ex "target remote localhost:$GDBPORT" \
        -ex "source $HERE/debug.gdb"
    echo "==> GDB exited; stopping QEMU (guest serial output was logged to $LOG)"
    ;;
attach)
    gdb -q \
        -ex "file $ELF" \
        -ex "target remote localhost:$GDBPORT" \
        -ex "source $HERE/debug.gdb"
    ;;
run)
    exec qemu-system-riscv64 "${QEMU_ARGS[@]}" -serial mon:stdio
    ;;
*)
    echo "Unknown mode: $MODE (use gdb | run | attach)" >&2
    exit 1
    ;;
esac
