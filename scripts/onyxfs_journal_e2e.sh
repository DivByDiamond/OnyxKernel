#!/usr/bin/env bash
# Legacy redo compatibility and full-block undo controls, booted in QEMU.
set -euo pipefail
exec python3 "$(dirname "$0")/qa/journal_recovery.py" "$@"
