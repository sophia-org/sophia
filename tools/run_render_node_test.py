#!/usr/bin/env python3
"""Run one frozen offscreen test with exactly one PCI GPU's render node.

Trusted host tooling, not an authorization boundary against a hostile same-UID
host process. The namespace confines the test and its descendants. This never
grants a primary DRM node, physical input, VT, or an existing display socket.
Test exit zero is not proof that it rendered: retain the test's own assertions.
"""
import argparse
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import signal
import stat
import subprocess
import sys
import time


PCI = re.compile(r"[0-9a-f]{4}:[0-9a-f]{2}:[0-9a-f]{2}\.[0-7]")
NAMESPACES = ("mnt", "pid", "net", "ipc", "uts", "user")


def digest(path):
    with open(path, "rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n")


def device_stat(node):
    return node.stat()


def select_device(pci, root=Path("/sys/class/drm"), dev=Path("/dev/dri")):
    if not PCI.fullmatch(pci):
        raise ValueError("PCI identity must be lower-case domain:bus:slot.function")
    matches = []
    for entry in sorted(root.glob("renderD*")):
        if not re.fullmatch(r"renderD[0-9]+", entry.name):
            continue
        physical = (entry / "device").resolve(strict=True)
        if physical.name != pci:
            continue
        major, minor = map(int, (entry / "dev").read_text().strip().split(":"))
        node = dev / entry.name
        info = device_stat(node)
        if (not stat.S_ISCHR(info.st_mode) or os.major(info.st_rdev) != major
                or os.minor(info.st_rdev) != minor):
            raise ValueError("render node disagrees with sysfs")
        matches.append({"pci": pci, "node": str(node), "major": major,
                        "minor": minor, "inode": info.st_ino, "filesystem": info.st_dev,
                        "vendor": (physical / "vendor").read_text().strip(),
                        "device": (physical / "device").read_text().strip()})
    if len(matches) != 1:
        raise ValueError(f"expected one render node for {pci}, found {len(matches)}")
    return matches[0]


def namespace_ids():
    return {name: os.readlink(f"/proc/self/ns/{name}") for name in NAMESPACES}


def verify_namespaces(host, observed):
    if any(observed[name] == host[name] for name in NAMESPACES):
        raise ValueError("all required namespaces must be private")


def command(bwrap, runner, run, device):
    # Start empty. In particular, never bind the host root, home, /run or /tmp.
    result = [str(bwrap), "--unshare-all", "--unshare-user", "--die-with-parent", "--new-session",
              "--disable-userns", "--assert-userns-disabled",
              "--cap-drop", "ALL", "--clearenv", "--tmpfs", "/",
              "--ro-bind", "/usr", "/usr",
              "--symlink", "usr/bin", "/bin", "--symlink", "usr/lib", "/lib",
              "--symlink", "usr/lib", "/lib64", "--dir", "/etc"]
    if Path("/etc/ld.so.cache").is_file():
        result += ["--ro-bind", "/etc/ld.so.cache", "/etc/ld.so.cache"]
    if Path("/nix/store").is_dir():
        result += ["--ro-bind", "/nix/store", "/nix/store"]
    result += ["--ro-bind", "/sys", "/sys", "--proc", "/proc", "--dev", "/dev",
               "--dir", "/dev/dri", "--dev-bind", device["node"], device["node"],
               "--tmpfs", "/tmp", "--tmpfs", "/run", "--dir", "/run/test",
               "--tmpfs", "/home", "--dir", "/home/test", "--dir", "/work",
               "--ro-bind", str(runner), "/work/runner.py",
               "--ro-bind", str(run / "test"), "/work/test",
               "--ro-bind", str(run / "config.json"), "/work/config.json",
               "--bind", str(run / "artifacts"), "/results", "--chdir", "/tmp"]
    environment = {"PATH": "/usr/bin:/bin", "HOME": "/home/test", "LANG": "C.UTF-8",
                   "XDG_RUNTIME_DIR": "/run/test", "PYTHONDONTWRITEBYTECODE": "1",
                   "SOPHIA_TEST_RENDER_NODE": device["node"], "RUST_TEST_THREADS": "1"}
    for name, value in environment.items():
        result += ["--setenv", name, value]
    return result + ["--", "/usr/bin/python3", "-B", "/work/runner.py", "--inside"]


def audit(config):
    observed = namespace_ids()
    verify_namespaces(config["host_namespaces"], observed)
    if select_device(config["device"]["pci"]) != config["device"]:
        raise ValueError("device identity changed across the namespace boundary")
    nodes = sorted(str(p) for p in Path("/dev/dri").iterdir())
    if nodes != [config["device"]["node"]]:
        raise ValueError("unexpected GPU nodes in sandbox")
    denied = ("/dev/input", "/dev/tty0", "/dev/tty7", "/dev/dri/card0",
              "/dev/dri/card1", "/tmp/.X11-unix", "/run/user", "/run/dbus",
              "/run/seatd.sock")
    if any(Path(p).exists() for p in denied):
        raise ValueError("display/input/session path visible in sandbox")
    if any(name in os.environ for name in ("DISPLAY", "WAYLAND_DISPLAY", "WAYLAND_SOCKET",
                                           "XAUTHORITY", "DBUS_SESSION_BUS_ADDRESS",
                                           "SSH_AUTH_SOCK", "LD_PRELOAD", "LD_LIBRARY_PATH")):
        raise ValueError("ambient display or loader environment reached sandbox")
    if sorted(p.name for p in Path("/home").iterdir()) != ["test"]:
        raise ValueError("host home visible in sandbox")
    inherited = []
    for entry in Path("/proc/self/fd").iterdir():
        try:
            if int(entry.name) > 2:
                inherited.append((entry.name, os.readlink(entry)))
        except FileNotFoundError:
            pass  # The directory iterator's descriptor has already closed.
    if inherited:
        raise ValueError("unexpected inherited descriptors")
    if digest("/work/test") != config["sha256"]:
        raise ValueError("test bytes changed")
    # Open the one delegated device. There is no software fallback in admission.
    fd = os.open(config["device"]["node"], os.O_RDWR | os.O_CLOEXEC)
    os.close(fd)
    os.chmod("/run/test", 0o700)
    return {"status": "admitted", "namespaces": observed, "device": config["device"],
            "visible_gpu_nodes": nodes, "absent_paths": denied,
            "inherited_descriptors_above_stdio": inherited}


def inside():
    config = json.loads(Path("/work/config.json").read_text())
    try:
        admission = audit(config)
        write_json(Path("/results/admission.json"), admission)
    except (OSError, ValueError, KeyError) as error:
        write_json(Path("/results/admission.json"), {"status": "refused", "reason": str(error)})
        return 2
    os.execv("/work/test", ["/work/test", *config["arguments"]])


def acquire_lock(pci):
    root = Path.home() / ".cache/sophia-render-tests/locks"
    root.mkdir(parents=True, exist_ok=True, mode=0o700)
    if root.is_symlink() or root.stat().st_uid != os.getuid():
        raise ValueError("lock directory is not owned by this user")
    fd = os.open(root / pci, os.O_RDWR | os.O_CREAT | os.O_CLOEXEC | os.O_NOFOLLOW, 0o600)
    try:
        fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BaseException:
        os.close(fd)
        raise
    return fd


def run(args):
    if not 0 < args.timeout <= 120:
        raise ValueError("timeout must be in (0, 120] seconds")
    if not re.fullmatch(r"[0-9a-f]{64}", args.sha256):
        raise ValueError("supply the frozen test's sha256")
    argv = args.command[1:] if args.command[:1] == ["--"] else args.command
    if not argv or not Path(argv[0]).is_absolute():
        raise ValueError("test executable must be an absolute path")
    binary = Path(argv[0]).resolve(strict=True)
    if not binary.is_file() or not os.access(binary, os.X_OK):
        raise ValueError("test executable must be a regular executable file")
    if digest(binary) != args.sha256:
        raise ValueError("frozen executable hash mismatch")
    device = select_device(args.pci)
    bwrap = shutil.which("bwrap")
    if bwrap is None:
        raise ValueError("bubblewrap is required; no unconfined fallback")
    runner = Path(__file__).resolve()
    output = args.output.resolve()
    output.mkdir(mode=0o700)  # An existing receipt must never be overwritten.
    (output / "artifacts").mkdir(mode=0o700)
    result = {"status": "refused", "device": device, "test_sha256": args.sha256,
              "runner_sha256": digest(runner), "command": argv, "timeout_seconds": args.timeout}
    lock_fd = None
    child = None
    started = time.monotonic()
    try:
        lock_fd = acquire_lock(args.pci)
        shutil.copyfile(runner, output / "runner.py")
        (output / "runner.py").chmod(0o400)
        if digest(output / "runner.py") != result["runner_sha256"]:
            raise ValueError("runner changed while freezing it")
        shutil.copyfile(binary, output / "test")
        (output / "test").chmod(0o500)
        if digest(output / "test") != args.sha256:
            raise ValueError("test changed while freezing it")
        config = {"device": device, "sha256": args.sha256, "arguments": argv[1:],
                  "host_namespaces": namespace_ids()}
        write_json(output / "config.json", config)
        launch = command(bwrap, output / "runner.py", output, device)
        write_json(output / "launch.json", launch)
        with (output / "stdout.log").open("xb") as stdout, (output / "stderr.log").open("xb") as stderr:
            child = subprocess.Popen(launch, stdin=subprocess.DEVNULL, stdout=stdout, stderr=stderr,
                                     env={"PATH": "/usr/bin:/bin"}, close_fds=True,
                                     start_new_session=True)
            try:
                status = child.wait(timeout=args.timeout)
                result.update(status="exited", exit=status)
            except subprocess.TimeoutExpired:
                result.update(status="timeout", exit=124)
            finally:
                if child.poll() is None:
                    os.killpg(child.pid, signal.SIGKILL)
                    try:
                        child.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        result.update(status="cleanup_incomplete", exit=125)
        admission = output / "artifacts/admission.json"
        result["admission"] = json.loads(admission.read_text()) if admission.exists() else None
        if result["status"] == "exited" and result["exit"] == 0:
            if not result["admission"] or result["admission"].get("status") != "admitted":
                raise ValueError("zero exit without device admission")
            if select_device(args.pci) != device:
                raise ValueError("device identity changed during test")
            result["status"] = "completed"
    except (OSError, ValueError, KeyboardInterrupt) as error:
        result.update(status="refused", reason=str(error), exit=2)
    finally:
        result["elapsed_seconds"] = time.monotonic() - started
        if lock_fd is not None:
            os.close(lock_fd)
        write_json(output / "result.json", result)
    print(json.dumps(result, sort_keys=True))
    return result["exit"]


def main():
    if sys.argv[1:] == ["--inside"]:
        return inside()
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pci", required=True)
    parser.add_argument("--sha256", required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--timeout", type=float, default=60)
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    try:
        return run(args)
    except (OSError, ValueError) as error:
        print(f"render test refused: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
