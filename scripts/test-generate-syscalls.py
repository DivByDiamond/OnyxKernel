#!/usr/bin/env python3
"""Registry validation and stale-consumer regression tests (host only)."""

import importlib.util
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).with_name("generate-syscalls.py")
SPEC = importlib.util.spec_from_file_location("generate_syscalls", SCRIPT)
GENERATOR = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(GENERATOR)


class RegistryTests(unittest.TestCase):
    def test_duplicate_number_or_name_is_rejected(self):
        for rows in ["1,write,\n1,read,\n", "1,write,\n2,write,\n"]:
            with self.subTest(rows=rows), tempfile.TemporaryDirectory() as directory:
                table = Path(directory) / "syscalls.csv"
                table.write_text("number,name,description\n" + rows)
                with self.assertRaisesRegex(ValueError, "duplicate syscall"):
                    GENERATOR.load_table(table)

    def test_zero_and_invalid_identifiers_are_rejected(self):
        for row in ["0,write,\n", "1,bad-name,\n", "18446744073709551616,write,\n"]:
            with self.subTest(row=row), tempfile.TemporaryDirectory() as directory:
                table = Path(directory) / "syscalls.csv"
                table.write_text("number,name,description\n" + row)
                with self.assertRaisesRegex(ValueError, "invalid syscall"):
                    GENERATOR.load_table(table)

    def test_check_detects_stale_exports_without_modifying_them(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            kernel, shell, compiler = [root / name for name in ("kernel", "shell", "compiler")]
            for path in [kernel / "scripts", kernel / "abi", kernel / "kernel/src/syscall",
                         kernel / "init/src/syscalls", shell / "src/syscalls",
                         compiler / "include/sys"]:
                path.mkdir(parents=True)
            shutil.copyfile(SCRIPT, kernel / "scripts/generate-syscalls.py")
            (kernel / "abi/syscalls.csv").write_text("number,name,description\n22,kill,signal\n62,getpgid,group\n")
            command = ["python3", str(kernel / "scripts/generate-syscalls.py"),
                       "--shell-dir", str(shell), "--compiler-dir", str(compiler)]
            subprocess.run(command, check=True, capture_output=True)
            subprocess.run(command + ["--check"], check=True, capture_output=True)
            exported = shell / "src/syscalls/numbers.rs"
            self.assertIn("SYS_kill: u64 = 22;", exported.read_text())
            exported.write_text(exported.read_text().replace("SYS_kill: u64 = 22;", "SYS_kill: u64 = 62;"))
            before = exported.read_bytes()
            result = subprocess.run(command + ["--check"], capture_output=True, text=True)
            self.assertEqual(result.returncode, 1)
            self.assertIn(str(exported), result.stderr)
            self.assertEqual(exported.read_bytes(), before)


if __name__ == "__main__":
    unittest.main()
