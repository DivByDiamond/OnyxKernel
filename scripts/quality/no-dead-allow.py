#!/usr/bin/env python3
"""Reject dead-code lint bypasses in tracked sources and build flags."""
from pathlib import Path
import re
import sys
import subprocess

root = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else Path(__file__).resolve().parents[2]
paths = subprocess.check_output(
    ["git", "-C", str(root), "ls-files", "--cached", "--others", "--exclude-standard", "-z", "*.rs", "*.toml", "Makefile", "*.mk", "*.cmake", "CMakeLists.txt", "*.sh", "*.yml", "*.yaml"], text=True
).split("\0")
patterns = [
    re.compile(r"#!?\[[^]]*\b(?:allow|expect)\s*\([^)]*\b(?:dead_code|unused|warnings)\b", re.S),
    re.compile(r"\bdead_code\s*=\s*[\"']allow[\"']"),
    re.compile(r"\bdead_code\s*=\s*\{[^}]*\blevel\s*=\s*[\"']allow[\"']"),
    re.compile(r"-A\s*(?:dead_code|dead-code|unused|warnings)\b"),
    re.compile(r"-Wno-unused-(?:function|variable|but-set-variable)\b"),
]
failed = False
for name in filter(None, paths):
    path = root / name
    if not path.is_file():
        continue  # A tracked dead scaffold may be removed in this patch.
    source = path.read_text()
    for pattern in patterns:
        for match in pattern.finditer(source):
            line = source.count("\n", 0, match.start()) + 1
            print(f"{name}:{line}: dead-code lint bypass")
            failed = True
if failed:
    raise SystemExit(1)
print("No dead-code lint bypasses in tracked sources/build flags")
