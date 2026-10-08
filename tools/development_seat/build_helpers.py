#!/usr/bin/env python3
"""Compile the fixed launch helpers into a fresh private directory; no install."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess


def build(output):
    source = Path(__file__).resolve().parent
    compiler = Path(shutil.which("cc")).resolve(strict=True)
    output.mkdir(mode=0o700)
    flags = ["-std=c11", "-O2", "-Wall", "-Wextra", "-Werror", "-D_FORTIFY_SOURCE=2",
             "-fstack-protector-strong", "-fPIE", "-pie", "-Wl,-z,relro,-z,now"]
    records = []
    for name, sources in (("custody-exec", ["custody_exec.c"]),
                          ("namespace-guard", ["namespace_guard.c"]),
                          ("sandbox-exec", ["sandbox_exec.c", "drop.c", "restrict.c"])):
        destination = output / name
        argv = [str(compiler), *flags, *(str(source / name) for name in sources), "-o", str(destination)]
        subprocess.run(argv, check=True)
        destination.chmod(0o500)
        records.append({"argv": argv, "path": str(destination),
                        "sha256": hashlib.sha256(destination.read_bytes()).hexdigest()})
    inputs = sorted(source.glob("*.c")) + sorted(source.glob("*.h"))
    receipt = {"compiler": str(compiler), "compiler_sha256": hashlib.sha256(compiler.read_bytes()).hexdigest(),
               "compiler_version": subprocess.check_output([str(compiler), "--version"], text=True),
               "inputs": {str(path): hashlib.sha256(path.read_bytes()).hexdigest() for path in inputs},
               "helpers": records, "installed": False}
    (output / "BUILD.json").write_text(json.dumps(receipt, sort_keys=True, indent=2) + "\n")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    build(args.output.resolve())
