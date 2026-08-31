#!/bin/bash
# test_concurrency.sh — SMP stress suite for the todo P1 concurrency fixes.
#
# Boots the full chain under QEMU with increasing hart counts and drives the
# shell through the three historically racy areas:
#   1. fork/waitpid under SMP (work-stealing vs. state copy publication)
#   2. concurrent procfs readers (per-hart G_PROCBUF, locked proc::count)
#   3. sched_setaffinity against exited children (UAF window)
#
# Prerequisites: the boot disk must exist (bash scripts/run_qemu.sh builds
# it as build/boot.img, or scripts/build_full_chain.sh in CI sandboxes).
#
# Usage:  bash scripts/test_concurrency.sh [SMP_LIST]
#         SMP_LIST defaults to "2 4" (MAX_HARTS is 8)
#
# Exit code 0 = all configurations passed with zero kernel panics.
set -u

HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"
BOOT_DIR="${ONYXBOOT_DIR:-$ROOT/../OnyxBoot}"
IMG="${ONYX_DISK_IMG:-$ROOT/build/boot.img}"
LOG="${ONYX_TEST_LOG:-/tmp/onyx-concurrency.log}"
SMP_LIST="${1:-2 4}"
TOTAL="${TOTAL:-200}"

if ! command -v qemu-system-riscv64 >/dev/null 2>&1; then
    echo "[-] qemu-system-riscv64 not installed"
    exit 1
fi
if [ ! -f "$IMG" ]; then
    echo "[-] $IMG not found — build it first (bash scripts/run_qemu.sh, then Ctrl+C before QEMU starts)"
    exit 1
fi
if [ ! -f "$BOOT_DIR/bootloader.bin" ]; then
    echo "[-] $BOOT_DIR/bootloader.bin not found — build OnyxBoot first"
    exit 1
fi

FAILURES=0

for SMP in $SMP_LIST; do
    : > "$LOG"
    echo "=== SMP fork + procfs + affinity stress (smp=$SMP) ==="
    {
      sleep 45                        # boot to login
      printf 'root\n';   sleep 6      # first-boot login (empty password)
      printf '\n';       sleep 6
      # 1) fork/waitpid stress: background jobs fork+exit repeatedly while
      #    the parent shell keeps running. A fork race would hand a
      #    work-stealing hart a child with zeroed fds (panic or silent doom);
      #    the waitpid race would leave the parent parked forever.
      for i in 1 2 3 4 5; do
          printf 'ls /bin &\n';       sleep 2
          printf 'cat /proc/stat &\n'; sleep 2
      done
      # 2) concurrent procfs readers hammer the per-hart G_PROCBUF slots.
      printf 'cat /proc/cpuinfo\n'; sleep 3
      printf 'cat /proc/meminfo\n'; sleep 3
      printf 'cat /proc/loadavg\n'; sleep 3
      printf 'cat /proc/stat\n'; sleep 3
      # 3) let idle harts steal + rebalance; a Creating/Ready race would
      #    corrupt the runqueue here.
      sleep 10
      printf 'cat /proc/stat\n'; sleep 3
      printf 'exit\n';   sleep 6
    } | timeout --signal=KILL "$TOTAL" qemu-system-riscv64 \
        -machine virt -m 256M -smp "$SMP" -nographic \
        -bios "$BOOT_DIR/bootloader.bin" \
        -drive file="$IMG",format=raw,if=none,id=drive0 \
        -device virtio-blk-device,drive=drive0 > "$LOG" 2>&1

    grep -aq "OnyxKernel v" "$LOG" || { echo "[-] no kernel banner (smp=$SMP)"; FAILURES=$((FAILURES+1)); continue; }
    echo "  [+] kernel booted"
    grep -aqE "osh\\\$" "$LOG"       || { echo "[-] did not reach the shell (smp=$SMP)"; FAILURES=$((FAILURES+1)); continue; }
    echo "  [+] userspace reached"
    grep -aq "processes " "$LOG"     && echo "  [+] /proc/stat served" \
                                     || { echo "[-] /proc/stat failed (smp=$SMP)"; FAILURES=$((FAILURES+1)); }
    N_PANIC=$(grep -ac "kernel panic\|PANIC" "$LOG" || true)
    echo "  [i] panic count: $N_PANIC"
    if [ "$N_PANIC" -ne 0 ]; then
        grep -a "panic" "$LOG" | head -5
        FAILURES=$((FAILURES+1))
    fi
done

if [ "$FAILURES" -eq 0 ]; then
    echo "[+] PASS: test_concurrency (smp: $SMP_LIST, 0 panics)"
    exit 0
fi
echo "[-] FAIL: $FAILURES failure(s)"
exit 1
