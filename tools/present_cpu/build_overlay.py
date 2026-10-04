#!/usr/bin/env python3
"""Reuse a guest's dependencies, replacing only init and the measured executables.

The appended newc archive overrides the base files at guest unpack time. The
base digest, overlay inputs and final image are recorded; no Cargo target or
entire guest filesystem is copied per run. Pass an existing CPU-capable image.
"""
import argparse
import gzip
import hashlib
import json
from pathlib import Path
import shutil
import re
import subprocess
from build_record import source_identity

ROOT = Path(__file__).resolve().parents[2]


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def build(args):
    receipt = json.loads(args.build_record.read_text())
    if receipt["status"] != "PASS" or receipt["source_before"] != receipt["source_after"]:
        raise ValueError("build record failed or source changed during compilation")
    for name, binary in (("sophia", args.sophia), ("client", args.client)):
        if digest(binary) != receipt["files"][name]["sha256"]:
            raise ValueError(f"{name} differs from build record")
    args.out.mkdir(parents=True, exist_ok=False)
    stage = args.out / "overlay"
    (stage / "usr/bin").mkdir(parents=True)
    (stage / "sbin").mkdir()
    files = {"sophia": (args.sophia, "usr/bin/sophia"),
             "client": (args.client, "usr/bin/present_cpu_workload"),
             "init": (ROOT / "tools/qemu_guest_init.sh", "sbin/sophia-qemu-init"),
             "export": (ROOT / "tools/present_cpu/export_guest_log.sh", "usr/bin/sophia-cpu-export"),
             "cat": (Path(shutil.which("cat")), "usr/bin/cat")}
    files.update({name: (Path(shutil.which(name)), "usr/bin/" + name)
                  for name in ("gzip", "base64", "sha256sum", "wc", "du")})
    record = {}
    for name, (source, guest) in files.items():
        target = stage / guest
        shutil.copy2(source, target)
        target.chmod(0o755)
        record[name] = {"path": str(source.resolve()), "guest_path": "/" + guest,
                        "sha256": digest(target)}
        if name in ("sophia", "client"):
            notes = subprocess.check_output(["readelf", "-n", str(target)], text=True)
            build_id = re.search(r"Build ID:\s*(\w+)", notes)
            if not build_id:
                raise ValueError(f"{name}: no ELF build ID")
            record[name]["elf_build_id"] = build_id.group(1)
    # Some dracut bases resolve /sbin through /usr/sbin. Supply both aliases.
    (stage / "usr/sbin").mkdir()
    shutil.copy2(stage / "sbin/sophia-qemu-init", stage / "usr/sbin/sophia-qemu-init")
    shutil.copy2(stage / "sbin/sophia-qemu-init", stage / "usr/bin/sophia-qemu-init")
    paths = sorted(str(p.relative_to(stage)) for p in stage.rglob("*"))
    archive = subprocess.run(["cpio", "--null", "-o", "-H", "newc", "--owner=0:0", "--reproducible"],
                             cwd=stage, input=("\0".join(paths) + "\0").encode(),
                             stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=True).stdout
    image = args.out / "guest.img"
    shutil.copyfile(args.base, image)
    with image.open("ab") as stream:
        stream.write(gzip.compress(archive, mtime=0))
    for name, path in (("base", args.base), ("initramfs", image), ("kernel", args.kernel)):
        record[name] = {"path": str(path.resolve()), "sha256": digest(path)}
    for name in ("sophia", "client"):
        if record[name]["sha256"] != receipt["files"][name]["sha256"]:
            raise ValueError(f"{name} changed while packaging")
    manifest = {"schema": 1, "files": record, "source": receipt["source_before"],
                "packaging_source": source_identity(), "build_record": receipt,
                "build_record_sha256": digest(args.build_record)}
    Path(str(image) + ".json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(json.dumps(manifest, indent=2))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    for key in ("base", "sophia", "client", "kernel", "out"):
        parser.add_argument("--" + key, type=Path, required=True)
    parser.add_argument("--build-record", type=Path, required=True)
    build(parser.parse_args())
