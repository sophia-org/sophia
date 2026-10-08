#!/usr/bin/env python3
"""Root login worker. Lifetime never exceeds the outer owner's deadline."""
import json
import os
from pathlib import Path
import select
import signal
import subprocess
import sys
import time

if __name__ == "__main__" and not globals().get("__bundle_verified__"):
    raise ValueError("worker must enter through the verified bootstrap")
sys.path.insert(0, str(Path(__file__).resolve().parent))
import config as configuration
import custody
import inventory
import login
import sandbox
from pam import Pam


def write(path, value):
    # Only trusted worker/owner can reach this directory on the host.
    with path.open("x") as stream:
        stream.write(json.dumps(value, sort_keys=True, indent=2) + "\n")


def read_admission(descriptor, child, deadline):
    data = b""
    while b"\n" not in data:
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise TimeoutError("admission deadline expired")
        ready = select.select([descriptor, child.pidfd], [], [], min(remaining, 0.1))[0]
        if descriptor in ready:
            chunk = os.read(descriptor, 16385 - len(data))
            data += chunk
            if len(data) > 16384:
                raise ValueError("oversized admission")
            if not chunk:
                break
        elif child.pidfd in ready:
            raise ValueError("sandbox exited before admission")
    if not data.endswith(b"\n") or data.count(b"\n") != 1:
        raise ValueError("missing or multiple admission records")
    row = json.loads(data)
    if row.get("schema") != 1 or row.get("status") != "admitted":
        raise ValueError(f"sandbox admission refused: {row}")
    return row


def main():
    config = configuration.load()
    run = Path(sys.argv[1])
    configuration.trusted(run)
    if run.parent != Path(config["output_root"]):
        raise ValueError("unexpected run directory")
    declaration = json.loads((run / "declaration.json").read_text())
    deadline = declaration["deadline"]
    if declaration["config"] != config or time.monotonic() >= deadline:
        raise ValueError("changed configuration or expired launch")
    result = {"schema": 1, "status": "failed"}
    pam = child = None
    read_fd = write_fd = None
    api = login.Login(config["tools"]["libelogind"]["path"])
    try:
        if api.session(os.getpid()) is not None:
            raise ValueError("worker inherited an existing login")
        pam = Pam(config["tools"]["libpam"]["path"], config["pam_service"], config["user"],
                  {"XDG_SEAT": config["seat"], "XDG_SESSION_TYPE": "wayland",
                   "XDG_SESSION_CLASS": "user"})
        pam.open()
        observed = login.attest(api, config)
        if pam.getenv("XDG_SESSION_ID") != observed["session"]:
            raise ValueError("PAM session id differs from own-process login")
        if pam.getenv("XDG_SEAT") != config["seat"]:
            raise ValueError("PAM seat differs from dedicated seat")
        write(run / "login.json", observed)
        devices = inventory.discover(config)
        if devices != declaration["inventory"]:
            raise ValueError("seat device inventory changed after login")
        row = {"config": config, "login": observed, "inventory": devices, "bus": declaration["bus"],
               "namespaces": {name: os.readlink(f"/proc/self/ns/{name}") for name in sandbox.NAMESPACES}}
        write(run / "inside.json", row)
        # Read access for the dropped auditor is provided by a read-only bind;
        # host parent directory stays 0700 root-owned.
        (run / "inside.json").chmod(0o444)
        for name in ("home", "runtime", "artifacts"):
            path = run / name
            path.mkdir(mode=0o700)
            os.chown(path, config["uid"], config["gid"])
        (run / "passwd").write_text(f"root:x:0:0:root:/root:/sbin/nologin\n{config['user']}:x:{config['uid']}:{config['gid']}::/home/development:/sbin/nologin\n")
        (run / "group").write_text(f"root:x:0:\n{config['user']}:x:{config['gid']}:\n")
        for name in ("passwd", "group"):
            (run / name).chmod(0o444)
        read_fd, write_fd = os.pipe2(os.O_CLOEXEC)
        argv = sandbox.command(config, run, observed, devices, write_fd, nix_store=Path("/nix/store").is_dir())
        with (run / "untrusted-session-output.log").open("xb") as output:
            guarded = [config["tools"]["guard"]["path"], "--", *argv]
            with custody.defer_term():
                child = custody.Child(custody.command(config, guarded), stdin=subprocess.DEVNULL,
                                      stdout=output, stderr=output, pass_fds=(write_fd,),
                                      env={"PATH": "/usr/bin:/bin", "LANG": "C.UTF-8"})
            os.close(write_fd)
            write_fd = None
            admission = read_admission(read_fd, child, deadline)
            if admission["session"] != observed["session"] or admission["inventory"] != devices:
                raise ValueError("admission identity changed")
            write(run / "admission.json", admission)
            os.close(read_fd)
            read_fd = None
            child.wait_until(deadline)
            result["session_exit"] = child.finish()
            child = None
            if result["session_exit"] != 0:
                raise ValueError("Session exited unsuccessfully")
        result["status"] = "completed"
    except BaseException as error:
        result["reason"] = str(error)
    finally:
        # A second TERM cannot interrupt reap, PAM teardown or the receipt.
        # The independent service SIGKILL bound remains in force.
        signal.pthread_sigmask(signal.SIG_BLOCK, {signal.SIGTERM})
        reaped = child is None
        if child is not None:
            try:
                result["cleanup_exit"] = child.finish(terminate=True)
                reaped = True
            except BaseException as error:
                result["status"], result["cleanup_error"] = "failed", str(error)
        for descriptor in (read_fd, write_fd):
            if descriptor is not None:
                os.close(descriptor)
        if pam is not None and reaped:
            try:
                pam.close()
            except BaseException as error:
                result["status"], result["pam_cleanup_error"] = "failed", str(error)
        elif pam is not None:
            # Never claim ordered teardown when the child cannot be reaped.
            # Worker exit closes the lifetime fd and triggers PDEATHSIG; their
            # relative completion, including a D-state task, is not guaranteed.
            result["pam_cleanup"] = "skipped_unreaped_child"
        write(run / "worker-result.json", result)
    return 0 if result["status"] == "completed" else 2


if __name__ == "__main__":
    os.umask(0o077)
    # A TERM asks the trusted worker to clean up. SIGKILL is covered by each
    # child edge's PDEATHSIG and by PID-namespace-init death.
    signal.signal(signal.SIGTERM, lambda *_: (_ for _ in ()).throw(InterruptedError("worker TERM")))
    sys.exit(main())
