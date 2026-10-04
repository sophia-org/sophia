#!/usr/bin/env python3
"""Build both measured binaries and bind their hashes to an unchanged source tree."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[2]


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def source_identity():
    git = lambda *args: subprocess.check_output(["git", "-C", str(ROOT), *args])
    return {"head": git("rev-parse", "HEAD").decode().strip(),
            "tracked_diff_sha256": hashlib.sha256(git("diff", "HEAD", "--binary")).hexdigest(),
            "untracked": {p: digest(ROOT / p) for p in git("ls-files", "--others", "--exclude-standard")
                          .decode().splitlines()}}


def build_environment():
    """Run inside the build wrapper; record configuration hashes, never credentials."""
    names = {"CARGO_HOME", "CARGO_TARGET_DIR", "CARGO_INCREMENTAL", "RUSTUP_TOOLCHAIN",
             "RUSTC", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER", "RUSTFLAGS",
             "CARGO_ENCODED_RUSTFLAGS", "RUSTDOCFLAGS", "CARGO_ENCODED_RUSTDOCFLAGS",
             "CC", "CXX", "CFLAGS", "CXXFLAGS", "AR", "LDFLAGS"}
    environment = {k: v for k, v in os.environ.items() if k in names or
                   k.startswith(("CARGO_PROFILE_", "CARGO_BUILD_", "CARGO_TARGET_"))}
    directories = [ROOT / ".cargo", *(p / ".cargo" for p in ROOT.parents),
                   Path(os.environ.get("CARGO_HOME", str(Path.home() / ".cargo")))]
    configs = {}
    for directory in directories:
        for name in ("config", "config.toml"):
            path = directory / name
            configs[str(path)] = ({"realpath": str(path.resolve()), "sha256": digest(path)}
                                  if path.exists() else None)
    return {"environment": environment, "configs": configs,
            "cargo": subprocess.check_output(["cargo", "-Vv"], text=True),
            "rustc": subprocess.check_output(["rustc", "-Vv"], text=True)}


def build(args):
    args.out.mkdir(parents=True, exist_ok=False)
    before = source_identity()
    record = {"source_before": before, "commands": [], "files": {}, "status": "RUNNING"}
    wrapper = [sys.executable, str(args.wrapper.resolve())] if args.wrapper else []
    environment_command = wrapper + [sys.executable, str(Path(__file__).resolve()), "--environment"]
    record["build_environment_before"] = json.loads(subprocess.check_output(environment_command, cwd=ROOT))
    record["wrapper"] = ({"path": str(args.wrapper.resolve()), "sha256": digest(args.wrapper)}
                         if args.wrapper else None)
    commands = [(["-p", "sophia-cli", "--features", "native-session"], "sophia"),
                (["-p", "sophia-session", "--features", "native-session", "--example", "present_cpu_workload"], "client")]
    files = {"sophia": args.target / "release/sophia",
             "client": args.target / "release/examples/present_cpu_workload"}
    for suffix, name in commands:
        command = wrapper + ["cargo", "build", "--release", "--offline", "--locked",
                             "--target-dir", str(args.target.resolve())] + suffix
        start = time.monotonic()
        with (args.out / (name + ".log")).open("w") as stream:
            result = subprocess.run(command, cwd=ROOT, stdout=stream, stderr=subprocess.STDOUT)
        record["commands"].append({"argv": command, "exit": result.returncode,
                                   "seconds": time.monotonic() - start})
        if not result.returncode:
            path = files[name]
            record["files"][name] = {"path": str(path.resolve()), "sha256": digest(path)}
        record["source_after"] = source_identity()
        record["status"] = "FAILED" if result.returncode or record["source_after"] != before else "RUNNING"
        (args.out / "build.json").write_text(json.dumps(record, indent=2) + "\n")
        if record["status"] == "FAILED":
            raise RuntimeError("build failed or source changed; see preserved build record")
    record["build_environment_after"] = json.loads(subprocess.check_output(environment_command, cwd=ROOT))
    record["final_hashes"] = {name: digest(path) for name, path in files.items()}
    unchanged = (record["build_environment_before"] == record["build_environment_after"] and
                 all(record["final_hashes"][name] == record["files"][name]["sha256"] for name in files))
    record["status"] = "PASS" if unchanged else "FAILED"
    (args.out / "build.json").write_text(json.dumps(record, indent=2) + "\n")
    if not unchanged:
        raise RuntimeError("build configuration or binaries changed; see preserved build record")
    print(args.out / "build.json", flush=True)


if __name__ == "__main__":
    if sys.argv[1:] == ["--environment"]:
        print(json.dumps(build_environment()))
        sys.exit(0)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--wrapper", type=Path, help="optional Python isolation launcher")
    build(parser.parse_args())
