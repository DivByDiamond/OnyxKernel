#!/usr/bin/env bash
# onyxfs_journal_e2e.sh — OnyxFS v2 journal crash-recovery e2e test.
#
# Automated replacement for the manual crash-recovery QEMU loop
# (todo.md "journal crash-recovery с реальным блочным I/O"). Two controls:
#
#   1. positive: inject a COMMITTED transaction (commit_end present) into
#      the journal area of a boot.img copy -> after a QEMU boot, mount()'s
#      journal_recover() must have replayed the payload into the target
#      data block and zeroed the journal.
#   2. negative: inject a TORN transaction (no commit_end) -> recovery
#      must discard it: target block unchanged, journal zeroed anyway.
#
# The injection/verification itself lives in onyxfs_journal_inject.c
# (build it first: cc -O2 -o scripts/onyxfs_journal_inject
#  scripts/onyxfs_journal_inject.c). The pre-injection data-block state is
# checked by the C tool; the shell driver boots QEMU and inspects the log.
#
# Usage: onyxfs_journal_e2e.sh [--pristine PATH] [--boot-dir PATH]
set -u

BOOT_DIR="$(cd "$(dirname "$0")/../.." && pwd)/OnyxBoot"
PRISTINE=""
LOGDIR="${ONYX_STRESS_DIR:-/tmp/onyx_stress}"
INJECT="$(dirname "$0")/onyxfs_journal_inject"

while [ $# -gt 0 ]; do
    case "$1" in
        --pristine) PRISTINE="$2"; shift 2 ;;
        --boot-dir) BOOT_DIR="$2"; shift 2 ;;
        --logdir)   LOGDIR="$2"; shift 2 ;;
        *)          shift ;;
    esac
done

KERNEL_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
if [ -z "$PRISTINE" ]; then
    PRISTINE="$KERNEL_ROOT/build/boot.img"
fi
if [ ! -f "$PRISTINE" ]; then
    echo "error: pristine image not found: $PRISTINE" >&2
    echo "       build it first: bash scripts/run_qemu.sh (or pass --pristine)" >&2
    exit 2
fi
if [ ! -x "$INJECT" ]; then
    echo "building $INJECT"
    cc -O2 -Wall -o "$INJECT" "$(dirname "$0")/onyxfs_journal_inject.c" || exit 2
fi

mkdir -p "$LOGDIR"

boot_and_wait() {  # $1 = disk, $2 = tag
    local disk="$1"
    local tag="$2"
    local log="$LOGDIR/jrnl_${tag}.log"
    timeout --signal=KILL 70 qemu-system-riscv64 \
        -machine virt -m 256M -smp 1 \
        -nographic -no-reboot \
        -bios "$BOOT_DIR/bootloader.bin" \
        -drive "file=$disk,format=raw,if=none,id=drive0" \
        -device virtio-blk-device,drive=drive0 \
        >"$log" 2>&1 </dev/null
    pkill -f "file=$disk" 2>/dev/null
    grep -aq "panicked" "$log" && echo "boot log: PANIC" || echo "boot log: no panic"
}

ok=1

echo "=== positive: COMMITTED transaction must replay ==="
pos="$LOGDIR/jrnl_pos.img"
cp "$PRISTINE" "$pos"
out=$("$INJECT" "$pos" --committed) || exit 2
echo "$out"
target=$(echo "$out" | awk '/^TARGET_BLOCK/ {print $2}')
boot_and_wait "$pos" pos
echo "[+] $("$INJECT" "$pos" --check --block "$target")"
"$INJECT" "$pos" --check --block "$target" | grep -q "replayed=1 journal_zeroed=1" || ok=0

echo "=== negative: TORN transaction must be discarded ==="
neg="$LOGDIR/jrnl_neg.img"
cp "$PRISTINE" "$neg"
out=$("$INJECT" "$neg" --torn) || exit 2
echo "$out"
target=$(echo "$out" | awk '/^TARGET_BLOCK/ {print $2}')
boot_and_wait "$neg" neg
echo "[-] $("$INJECT" "$neg" --check --block "$target")"
"$INJECT" "$neg" --check --block "$target" | grep -q "replayed=0 journal_zeroed=1" || ok=0

rm -f "$pos" "$neg"
echo
if [ "$ok" -eq 1 ]; then
    echo "=== JOURNAL E2E: PASS ==="
    exit 0
fi
echo "=== JOURNAL E2E: FAIL ==="
exit 1
