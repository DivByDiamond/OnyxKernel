#!/usr/bin/env bash
# smp_respawn_stress.sh — SMP respawn stress test for OnyxKernel.
#
# Runs N sequential QEMU boots (boot -> login as root -> `exit` from osh)
# and classifies each serial log against the known fault patterns. This is
# the harness that produced every stress number in todo.md's SMP crash
# investigation (baseline: 6/11 faulting under -smp 2, 3/3 clean under
# -smp 1; after the four-wave fix series: 12/12 and 6/6 clean).
#
# Usage: smp_respawn_stress.sh [N] [--smp K] [--pristine PATH] [--boot-dir PATH]
#
# Requires: qemu-system-riscv64, dd, grep. Logs land in $LOGDIR
# (default /tmp/onyx_stress); the pristine disk image is built by
# OnyxKernel/scripts/run_qemu.sh (build/boot.img) unless overridden.
set -u

N=10
SMP=2
BOOT_DIR="$(cd "$(dirname "$0")/../.." && pwd)/OnyxBoot"
PRISTINE=""
LOGDIR="${ONYX_STRESS_DIR:-/tmp/onyx_stress}"

while [ $# -gt 0 ]; do
    case "$1" in
        --smp)       SMP="$2"; shift 2 ;;
        --pristine)  PRISTINE="$2"; shift 2 ;;
        --boot-dir)  BOOT_DIR="$2"; shift 2 ;;
        --logdir)    LOGDIR="$2"; shift 2 ;;
        *)           N="$1"; shift ;;
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
if [ ! -f "$BOOT_DIR/bootloader.bin" ]; then
    echo "error: OnyxBoot bootloader.bin not found in $BOOT_DIR" >&2
    exit 2
fi

mkdir -p "$LOGDIR"

# Fault patterns: same set the Python harness matched (todo.md signatures).
FAULT_PATTERNS=(
    "trap: KERNEL page fault"
    "illegal instruction"
    "access fault"
    "klog::halt"
    "panicked at"
    "IDLESW"
)

clean=0
for i in $(seq 1 "$N"); do
    disk="$LOGDIR/run_${i}.img"
    log="$LOGDIR/run_${i}.log"
    cp "$PRISTINE" "$disk"

    t0=$(date +%s)
    # Feed the login script through a background subshell with sleeps, the
    # same timing the interactive smoke test uses (boot to login ~25 s).
    # The pipeline runs in a full subshell ( ... ) with its own stderr, so
    # QEMU's kill-related job notices never reach this script's stdout.
    (
        {
            sleep 25; printf 'root\n'
            sleep 8;  printf '\n'      # empty password (first boot)
            sleep 8;  printf 'exit\n'  # exit osh -> init respawn cycle
            sleep 15
        } | timeout --signal=KILL 90 qemu-system-riscv64 \
            -machine virt -m 256M -smp "$SMP" \
            -nographic -no-reboot \
            -bios "$BOOT_DIR/bootloader.bin" \
            -drive "file=$disk,format=raw,if=none,id=drive0" \
            -device virtio-blk-device,drive=drive0 \
            >"$log" 2>&1
    ) 2>/dev/null
    dt=$(( $(date +%s) - t0 ))

    # Kill any straggler QEMU bound to this run's disk.
    pkill -f "file=$disk" 2>/dev/null

    faults=""
    for pat in "${FAULT_PATTERNS[@]}"; do
        hits=$(grep -a -m 2 -F "$pat" "$log" | head -2)
        if [ -n "$hits" ]; then
            while IFS= read -r line; do
                faults="${faults}         | ${line:0:160}\n"
            done <<< "$hits"
        fi
    done

    if [ -n "$faults" ]; then
        status="FAULT"
    else
        status="CLEAN"
        clean=$((clean + 1))
    fi
    login_ok="False"
    if grep -aq "osh" "$log" && grep -aqi "login" "$log"; then
        login_ok="True"
    fi

    printf '[run %02d] %s login_ok=%s dt=%ss\n' "$i" "$status" "$login_ok" "$dt"
    if [ -n "$faults" ]; then
        printf '%b' "$faults"
    fi
    rm -f "$disk"
done

echo
echo "=== RESULT: $clean/$N clean under -smp $SMP ==="
[ "$clean" -eq "$N" ]
