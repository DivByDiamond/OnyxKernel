#!/usr/bin/env bash
# Full Rust quality gate: host tests/tools plus both bare-metal kernel ports.
set -euo pipefail
cd "$(dirname "$0")/../.."
python3 scripts/quality/no-dead-allow.py
cargo fmt --all -- --check
for variant in default smode; do
    features=()
    if [ "$variant" = smode ]; then features=(--all-features); fi
    cargo clippy -p onyx_core -p onyx_kernel -p onyx_tools --all-targets "${features[@]}" -- -D warnings
    targets=(riscv64gc-unknown-none-elf)
    if [ "$variant" = default ]; then targets+=(riscv32imac-unknown-none-elf); fi
    for target in "${targets[@]}"; do
        cargo clippy -p onyx_kernel --lib --bin onyx-kernel --release --target "$target" "${features[@]}" -- -D warnings
    done
done
# Freestanding init bins use RV64 assembly; their pure library is tested on host.
cargo clippy -p onyx_init --lib -- -D warnings
cargo clippy -p onyx_init --lib --bins --release --target riscv64gc-unknown-none-elf -- -D warnings

# RV32 has only an M-mode entry; reject the formerly unbootable zero-entry ELF.
python3 - <<'PYTHON'
import subprocess
import sys
result = subprocess.run(
    ["cargo", "check", "-p", "onyx_kernel", "--bin", "onyx-kernel", "--release",
     "--target", "riscv32imac-unknown-none-elf", "--features", "smode"],
    capture_output=True, text=True,
)
if result.returncode == 0 or "RV32 smode boot entry is not implemented" not in result.stderr:
    sys.stderr.write(result.stderr)
    raise SystemExit("RV32 smode must fail for the explicit unsupported boot entry")
print("Unsupported RV32 smode rejected explicitly")
PYTHON
