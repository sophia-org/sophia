#!/usr/bin/env python3
"""Build the private development-seat bubblewrap. Never download or install.

Supply the upstream v0.13.0 release tarball and a new output directory. All
dependencies and compiler identity still belong in the launch bundle receipt.
The executable is only for the restricted development mount namespace.
"""
import argparse
import hashlib
from pathlib import Path
import subprocess
import tarfile


ARCHIVE_SHA256 = "4734237473c0e5d695e4e9034a34e43b2dbf5164655bd13fa59ae376b2b7a765"


def build(archive, output):
    if hashlib.sha256(archive.read_bytes()).hexdigest() != ARCHIVE_SHA256:
        raise ValueError("bubblewrap release archive hash mismatch")
    output.mkdir(mode=0o700)
    with tarfile.open(archive) as contents:
        contents.extractall(output, filter="data")
    source = output / "bubblewrap-0.13.0"
    patch = Path(__file__).with_name("bubblewrap-no-loopback.patch")
    subprocess.run(["patch", "--batch", "--fuzz=0", "-p1", "-i", str(patch.resolve())],
                   cwd=source, check=True)
    destination = output / "build"
    subprocess.run(["meson", "setup", str(destination), str(source), "--buildtype=release",
                    "-Dtests=false", "-Dman=disabled", "-Dselinux=disabled",
                    "-Dbash_completion=disabled", "-Dzsh_completion=disabled"], check=True)
    subprocess.run(["ninja", "-C", str(destination), "-j2", "bwrap"], check=True)
    # Meson can embed the build user's libcap directory. That must never be a
    # privileged loader path. Use the host's trusted system library search;
    # record/requalify its libcap bytes with the final root-owned bundle.
    subprocess.run(["patchelf", "--remove-rpath", str(destination / "bwrap")], check=True)
    return destination / "bwrap"


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("archive", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    print(build(args.archive.resolve(), args.output.resolve()))
