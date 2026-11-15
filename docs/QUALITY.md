# Mandatory Rust quality checks

Run the complete gate after every change, including formatting-only work:

```sh
bash scripts/quality/check.sh
```

It checks all workspace formatting and rejects dead-code lint bypasses, then
runs strict Clippy (`-D warnings`) for core/kernel/tools including host tests,
for RV64 default/smode and RV32 default, and for init's
host library plus every RV64 executable. RV32 smode has no boot entry and is required to fail with an explicit
diagnostic; the gate tests that rejection too. Host libtest targets cannot be built
for bare-metal RISC-V; real binaries and their host tests are checked separately.

Do not narrow this to changed files, add a lint allowance to pass it, or turn a
failing CI check into an advisory check. Remove dead code, share live modules
through the existing library, or correct the implementation. Each executable
installs its own allocator; the shared init library must remain allocator-free.

From the ecosystem checkout, include Shell and all six dead-code scans:

```sh
cd ../OnyxOS
bash scripts/quality/check.sh /path/to/component/checkouts
```

The scanner covers tracked and new Rust/Cargo sources and Make/CMake flags, including
multiline `allow`/`expect`, blanket `unused`/`warnings`, and unused-function or
unused-variable warning suppression. Generated build products are not sources.

Behavior checks remain necessary: host tests, target builds and, for kernel/ABI
changes, both OnyxOS QEMU smoke scripts. Journal changes also require the
write-boundary host regressions and `scripts/onyxfs_journal_e2e.sh` against the
freshly built disk. Quality checks do not prove crash recovery by themselves.

The current toolchain separately diagnoses `+reserve-x4` as an unstable LLVM
backend feature while still forwarding it. Keep that setting: the kernel uses
`tp` for per-hart identity and requires LLVM to reserve it. No warning group is
disabled to hide this diagnostic; all Clippy commands still use `-D warnings`.
