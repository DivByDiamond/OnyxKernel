#!/usr/bin/env python3
"""Generate syscall numbers; committed outputs keep consumers standalone."""

import argparse
import csv
from pathlib import Path
import re
import sys


def load_table(path):
    with path.open(newline="", encoding="utf-8") as handle:
        reader = csv.DictReader(handle)
        if reader.fieldnames != ["number", "name", "description"]:
            raise ValueError("expected number,name,description columns")
        rows = list(reader)
    names, numbers = set(), set()
    for row in rows:
        number = int(row["number"])
        name = row["name"]
        if not 0 < number < 2**64 or not re.fullmatch(r"[a-z][a-z0-9_]*", name):
            raise ValueError(f"invalid syscall: {row}")
        if name in names or number in numbers:
            raise ValueError(f"duplicate syscall name or number: {row}")
        names.add(name)
        numbers.add(number)
    if not rows:
        raise ValueError("syscall table is empty")
    return sorted(rows, key=lambda row: int(row["number"]))


def rust_numbers(rows, uppercase=False):
    result = "// Generated from OnyxKernel/abi/syscalls.csv; do not edit.\n"
    for row in rows:
        name = row["name"].upper() if uppercase else row["name"]
        result += f"pub const SYS_{name}: u64 = {row['number']};\n"
    return result


def c_numbers(rows):
    result = "/* Generated from OnyxKernel/abi/syscalls.csv; do not edit. */\n"
    result += "#ifndef ONYX_SYSCALL_NUMBERS_H\n#define ONYX_SYSCALL_NUMBERS_H\n\n"
    for row in rows:
        result += f"#define SYS_{row['name']} {row['number']}\n"
    return result + "\n#endif /* ONYX_SYSCALL_NUMBERS_H */\n"


def main():
    kernel = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="fail on stale outputs")
    parser.add_argument("--shell-dir", type=Path, help="also export/check OnyxShell")
    parser.add_argument("--compiler-dir", type=Path, help="also export/check OnyxCompiller")
    args = parser.parse_args()
    try:
        rows = load_table(kernel / "abi/syscalls.csv")
    except (ValueError, KeyError, TypeError) as error:
        parser.error(str(error))
    outputs = {
        kernel / "kernel/src/syscall/numbers.rs": rust_numbers(rows),
        kernel / "init/src/syscalls/numbers.rs": rust_numbers(rows, uppercase=True),
    }
    if args.shell_dir:
        outputs[args.shell_dir / "src/syscalls/numbers.rs"] = rust_numbers(rows)
    if args.compiler_dir:
        outputs[args.compiler_dir / "include/sys/syscall_numbers.h"] = c_numbers(rows)
    stale = []
    for path, expected in outputs.items():
        if args.check:
            if not path.is_file() or path.read_text(encoding="utf-8") != expected:
                stale.append(str(path))
        else:
            path.write_text(expected, encoding="utf-8")
    if stale:
        print("Stale syscall numbers; regenerate:\n" + "\n".join(stale), file=sys.stderr)
        return 1
    print(f"{'Checked' if args.check else 'Generated'} {len(outputs)} ABI files ({len(rows)} syscalls)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
