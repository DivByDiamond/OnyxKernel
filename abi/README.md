# Syscall numbers

`syscalls.csv` is the authoritative syscall-number registry. Never renumber
existing entries or reuse retired numbers. Unassigned gaps remain reserved.
The table describes numbers, not access policy: permissions are enforced in
`kernel/src/syscall/handler/acl.rs` and individual syscall handlers.

Generate kernel/init outputs and export the consumer snapshots from this repo:

```sh
python3 scripts/generate-syscalls.py --shell-dir ../OnyxShell --compiler-dir ../OnyxCompiller
python3 scripts/generate-syscalls.py --check --shell-dir ../OnyxShell --compiler-dir ../OnyxCompiller
```

Use explicit paths for `.vent/repos/` or other checkout layouts. Without the
consumer options, the command only generates/checks this repository. Commit
the table and generated files together, including outputs in affected consumer
repositories. Consumers compile their checked-in snapshots without Python or a
kernel source dependency; run the cross-repository check before integration.

Generated Rust constants are included by the kernel, init and shell. The C
header `OnyxCompiller/include/sys/syscall_numbers.h` is included by libc's
syscall interface, the host runner, and compiler syscall fixtures. Other ABI
types, flags and calling conventions remain in their existing headers.
