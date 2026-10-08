#!/usr/bin/env python3
"""Verify the source bundle before importing any of its modules.

The interpreter, standard library and this initial source script are trusted
through root-owned deployment. -S disables site startup; /dev/null cannot hold
bytecode, so even an unchecked hash-based cache is never an import candidate.
"""
import hashlib
import json
import os
from pathlib import Path
import stat
import sys


PYTHON_FLAGS = ["-I", "-B", "-S", "-X", "pycache_prefix=/dev/null"]


def trusted(path, regular=False):
    path = Path(path)
    if not path.is_absolute() or ".." in path.parts:
        raise ValueError("bootstrap path must be absolute")
    for part in [*reversed(path.parents), path]:
        info = part.lstat()
        if info.st_uid != 0 or stat.S_ISLNK(info.st_mode) or info.st_mode & 0o022:
            raise ValueError(f"untrusted bootstrap path: {part}")
    if regular and (not stat.S_ISREG(info.st_mode) or info.st_mode & 0o6000):
        raise ValueError(f"expected ordinary bootstrap file: {path}")


def inventory(config):
    """Compare all entries, including directories, without following links."""
    root = Path(config["bundle"])
    pins = {}
    for pin in [*config["tools"].values(), *config["files"]]:
        path = Path(pin["path"])
        if not path.is_relative_to(root):
            continue
        if str(path) != pin["path"] or ".." in path.parts or path == root:
            raise ValueError("noncanonical bundle pin")
        if path in pins and pins[path] != pin["sha256"]:
            raise ValueError("conflicting bundle pin")
        pins[path] = pin["sha256"]
    directories = {parent for path in pins for parent in path.parents
                   if parent != root and parent.is_relative_to(root)}
    expected = set(pins) | directories
    actual = set(root.rglob("*"))
    if actual != expected:
        raise ValueError("bundle directory inventory differs from pins")
    for path in actual:
        info = path.lstat()
        if (stat.S_ISLNK(info.st_mode) or path.suffix in (".pyc", ".pyo") or
                "__pycache__" in path.relative_to(root).parts):
            raise ValueError("bundle contains a link or bytecode cache")
        if path in directories:
            if not stat.S_ISDIR(info.st_mode):
                raise ValueError("bundle parent is not a directory")
        elif not stat.S_ISREG(info.st_mode):
            raise ValueError("bundle pin is not a regular file")
        elif hashlib.sha256(path.read_bytes()).hexdigest() != pins[path]:
            raise ValueError("bundle file hash differs from pin")
    return pins


def command(config, role, *args):
    if role not in ("owner", "worker", "audit"):
        raise ValueError("unknown bootstrap role")
    return [config["tools"]["python"]["path"], *PYTHON_FLAGS,
            str(Path(config["bundle"]) / "bootstrap.py"), role, *args]


def main():
    if (not sys.flags.isolated or not sys.flags.no_site or not sys.dont_write_bytecode
            or sys.pycache_prefix != "/dev/null"):
        raise ValueError("required Python startup restrictions missing")
    null = Path("/dev/null").lstat()
    if not stat.S_ISCHR(null.st_mode) or null.st_rdev != os.makedev(1, 3):
        raise ValueError("bytecode prefix must be the null device")
    if len(sys.argv) < 2 or sys.argv[1] not in ("owner", "worker", "audit"):
        raise ValueError("unknown bootstrap role")
    role = sys.argv[1]
    source = Path(__file__).absolute()
    trusted(source, regular=True)
    settings = Path("/run/inside.json" if role == "audit" else "/etc/sophia-development/launch.json")
    trusted(settings, regular=True)
    value = json.loads(settings.read_text())
    config = value["config"] if role == "audit" else value
    if Path(config["bundle"]) != source.parent:
        raise ValueError("bootstrap is outside the configured bundle")
    pins = inventory(config)
    for path in pins:
        trusted(path, regular=True)
    entry = source.parent / (role + ".py")
    if source not in pins or entry not in pins:
        raise ValueError("bootstrap and entry source pins required")
    sys.argv = [str(entry), *sys.argv[2:]]
    # Execute verified source directly; imports begin only after this point.
    exec(compile(entry.read_bytes(), str(entry), "exec"),
         {"__name__": "__main__", "__file__": str(entry), "__bundle_verified__": True})


if __name__ == "__main__":
    main()
