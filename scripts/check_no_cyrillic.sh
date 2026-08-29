#!/usr/bin/env bash
# check_no_cyrillic.sh — fail if any Cyrillic (non-ASCII) characters appear
# in source files. Project rule: ALL code comments must be English ASCII
# (todo.md rule set: "Translate all code comments to English").
#
# Usage: scripts/check_no_cyrillic.sh [root-dir]   (default: repo root)
# Exits 0 when clean, 1 with a list of offending file:line entries.

set -euo pipefail

ROOT="${1:-$(cd "$(dirname "$0")/.." && pwd)}"

EXTENSIONS='rs c h cpp cc hpp sh py mk toml ld S s in json yml yaml'

# file lists for grep must be NUL-safe; use --include filters per extension
INCLUDES=()
for e in $EXTENSIONS; do
    INCLUDES+=(--include="*.${e}")
    INCLUDES+=(--include="*.${e}.*")
done

MATCHES=$(grep -rnP '[\x{0400}-\x{04FF}]' "${INCLUDES[@]}" \
    --exclude-dir=.git --exclude-dir=target --exclude-dir=build \
    --exclude-dir=.build --exclude-dir=node_modules \
    "$ROOT" 2>/dev/null || true)

if [ -n "$MATCHES" ]; then
    echo "[-] Cyrillic characters found in source files:"
    echo "$MATCHES"
    exit 1
fi

echo "[+] OK: no Cyrillic characters in source files under $ROOT"
