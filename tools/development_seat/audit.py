#!/usr/bin/env python3
"""Trusted last step after credential drop, before the development executable."""
import errno
import json
import os
from pathlib import Path
import resource
import socket
import stat
import sys

# This file is executed only from the pinned, read-only bundle. -I excludes
# user/site paths; add precisely that directory, not cwd or PYTHONPATH.
sys.path.insert(0, str(Path(__file__).resolve().parent))
from sandbox import NAMESPACES, environment, session_command
from config import digest


def verify_files(config):
    bundle = Path(config["bundle"])
    for pin in [*config["tools"].values(), *config["files"]]:
        # Every bundle file, including the profile, is mandatory. Host-only
        # PAM files need not be mounted into the dropped process.
        path = Path(pin["path"])
        required = path.is_relative_to(bundle)
        if (required or path.is_file()) and digest(path) != pin["sha256"]:
            raise ValueError("bundle changed at the boundary")
    for key in ("sophia", "drop", "python", "nested_bubblewrap"):
        pin = config["tools"][key]
        if digest(Path(pin["path"])) != pin["sha256"]:
            raise ValueError(f"mandatory executable changed: {key}")


def check(row, admission_fd):
    config = row["config"]
    uid, gid = config["uid"], config["gid"]
    if os.getresuid() != (uid, uid, uid) or os.getresgid() != (gid, gid, gid) or os.getgroups():
        raise ValueError("credential transition incomplete")
    status = dict(line.split(":", 1) for line in Path("/proc/self/status").read_text().splitlines())
    if any(int(status[key], 16) for key in ("CapInh", "CapPrm", "CapEff", "CapBnd", "CapAmb")):
        raise ValueError("a capability survived the drop")
    if int(status["NoNewPrivs"]) != 1 or int(status["Seccomp"]) != 2:
        raise ValueError("mandatory process restrictions missing")
    namespaces = {name: os.readlink(f"/proc/self/ns/{name}") for name in NAMESPACES}
    for name in NAMESPACES:
        equal = namespaces[name] == row["namespaces"][name]
        if equal != (name in ("net", "user")):
            raise ValueError(f"unexpected {name} namespace")
    if Path("/proc/self/cgroup").read_text() != row["login"]["process"]["cgroup"]:
        raise ValueError("sandbox left its registered login cgroup")
    expected_nodes = row["inventory"]["nodes"]
    if sorted(str(p) for p in Path("/dev/dri").iterdir()) != sorted(n["node"] for n in expected_nodes):
        raise ValueError("unexpected visible DRM nodes")
    for node in expected_nodes:
        info = Path(node["node"]).stat()
        if (not stat.S_ISCHR(info.st_mode) or info.st_rdev != os.makedev(node["major"], node["minor"])
                or (info.st_dev, info.st_ino) != (node["filesystem"], node["inode"])):
            raise ValueError("device identity changed at the boundary")
    for path in ("/dev/input", "/dev/tty0", "/run/seatd.sock", "/tmp/.X11-unix"):
        if Path(path).exists():
            raise ValueError(f"unexpected host input or display path: {path}")
    runtime = Path(environment(config, row["login"])["XDG_RUNTIME_DIR"]).stat()
    if runtime.st_uid != uid or stat.S_IMODE(runtime.st_mode) != 0o700:
        raise ValueError("private runtime directory ownership/mode")
    if dict(os.environ) != environment(config, row["login"]):
        raise ValueError("unexpected environment at exec boundary")
    bus = Path("/run/dbus/system_bus_socket").lstat()
    expected_bus = row["bus"]
    if (not stat.S_ISSOCK(bus.st_mode) or
            (bus.st_dev, bus.st_ino, bus.st_uid) !=
            (expected_bus["filesystem"], expected_bus["inode"], expected_bus["uid"])):
        raise ValueError("system bus socket changed at the boundary")
    verify_files(config)
    for family, protocol in ((socket.AF_INET, 0), (socket.AF_NETLINK, 0)):
        try:
            descriptor = socket.socket(family, socket.SOCK_DGRAM, protocol)
        except OSError as error:
            if error.errno != errno.EPERM:
                raise
        else:
            descriptor.close()
            raise ValueError("socket filter missing")
    with socket.socket(socket.AF_NETLINK, socket.SOCK_DGRAM, 15):
        pass
    # listdir's own descriptor has closed before existence checks.
    descriptors = {int(name) for name in os.listdir("/proc/self/fd")
                   if Path("/proc/self/fd", name).exists()}
    if descriptors != {0, 1, 2, admission_fd} or not stat.S_ISFIFO(os.fstat(admission_fd).st_mode):
        raise ValueError("unexpected inherited descriptor")
    if any(os.isatty(fd) for fd in (0, 1, 2)):
        raise ValueError("sandbox has a terminal")
    return {"schema": 1, "status": "admitted", "namespaces": namespaces,
            "uid": uid, "gid": gid, "session": row["login"]["session"],
            "inventory": row["inventory"], "fds": sorted(descriptors), "command": session_command(config)}


def main():
    fd = int(sys.argv[1])
    row = json.loads(Path("/run/inside.json").read_text())
    try:
        record = check(row, fd)
    except BaseException as error:
        record = {"schema": 1, "status": "refused", "reason": str(error)}
    with os.fdopen(fd, "w") as stream:
        stream.write(json.dumps(record, sort_keys=True) + "\n")
        stream.flush()
    if record["status"] != "admitted":
        return 2
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    command = session_command(row["config"])
    os.execve(command[0], command, environment(row["config"], row["login"]))


if __name__ == "__main__":
    sys.exit(main())
