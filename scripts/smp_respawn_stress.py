#!/usr/bin/env python3
"""SMP stress test for OnyxKernel: N sequential QEMU runs of boot -> login -> exit.

Mirrors the scenario from OnyxKernel/todo.md (Sept 4 SMP crash investigation):
boot with -smp 2, login as root, exit from osh, watch for faults.

Usage: stress.py <iterations> [--smp N] [--timeout SEC]
Writes per-run logs to /tmp/onyx_stress/run_<i>.log
"""
import pexpect
import subprocess
import sys
import time
import os
import re
import shutil

BOOT_DIR = "/storage/project/systems/Onyx/OnyxBoot"
PRISTINE = "/tmp/onyx_stress/pristine.img"
LOGDIR = "/tmp/onyx_stress"

FAULT_PATTERNS = [
    r"trap: KERNEL page fault",
    r"trap: .* page fault",
    r"illegal instruction",
    r"access fault",
    r"klog::halt",
    r"panicked at",
    r"IDLESW",
]

def classify(log):
    hits = []
    for pat in FAULT_PATTERNS:
        for m in re.finditer(pat, log):
            # context: grab the line
            line_start = log.rfind("\n", 0, m.start()) + 1
            line_end = log.find("\n", m.end())
            line = log[line_start:line_end if line_end > 0 else len(log)].strip()
            hits.append(line[:160])
    return hits

def one_run(i, smp, timeout):
    disk = f"{LOGDIR}/run_{i}.img"
    shutil.copyfile(PRISTINE, disk)
    logf = open(f"{LOGDIR}/run_{i}.log", "wb")
    t0 = time.time()
    proc = subprocess.Popen(
        ["qemu-system-riscv64", "-machine", "virt", "-m", "256M", "-smp", str(smp),
         "-nographic", "-no-reboot",
         "-bios", f"{BOOT_DIR}/bootloader.bin",
         "-drive", f"file={disk},format=raw,if=none,id=drive0",
         "-device", "virtio-blk-device,drive=drive0"],
        stdout=logf, stderr=logf, stdin=subprocess.PIPE,
        preexec_fn=os.setsid)
    script = [
        (25, b"root\n"),
        (8,  b"\n"),          # empty password (first boot)
        (8,  b"exit\n"),      # exit osh (which exits... per todo scenario)
    ]
    try:
        for delay, data in script:
            time.sleep(delay)
            proc.stdin.write(data)
            proc.stdin.flush()
        # let it settle after exit (crash usually happens here)
        time.sleep(15)
        rc = proc.poll()
        if rc is None:
            proc.kill()
        ok_killed = True
    finally:
        logf.close()
        try:
            proc.kill()
        except Exception:
            pass
        # make sure no qemu left
        subprocess.run(["pkill", "-f", f"file={disk}"], capture_output=True)
    dt = time.time() - t0
    with open(f"{LOGDIR}/run_{i}.log", "r", errors="replace") as f:
        log = f.read()
    faults = classify(log)
    got_shell = ("osh" in log) and ("login" in log.lower())
    return {"i": i, "dt": dt, "faults": faults, "login_ok": got_shell,
            "log": f"{LOGDIR}/run_{i}.log"}

def main():
    n = int(sys.argv[1]) if len(sys.argv) > 1 else 10
    smp = 2
    timeout = 0
    args = sys.argv[2:]
    j = 0
    while j < len(args):
        if args[j] == "--smp":
            smp = int(args[j+1]); j += 2
        else:
            j += 1
    clean = 0
    dirty = []
    for i in range(1, n+1):
        r = one_run(i, smp, timeout)
        status = "CLEAN" if not r["faults"] else "FAULT"
        if not r["faults"]:
            clean += 1
        else:
            dirty.append(r)
        print(f"[run {i:02d}] {status} login_ok={r['login_ok']} dt={r['dt']:.0f}s")
        for line in r["faults"][:4]:
            print(f"         | {line}")
        sys.stdout.flush()
    print(f"\n=== RESULT: {clean}/{n} clean under -smp {smp} ===")
    return 0 if clean == n else 1

if __name__ == "__main__":
    sys.exit(main())
