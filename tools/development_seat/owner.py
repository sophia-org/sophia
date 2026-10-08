#!/usr/bin/env python3
"""Fixed root service entry: --check observes only; --run owns one bounded login.

This source is not an installed launcher. A root-owned bundle, dedicated locked
account, reviewed host policy and PAM stack, seat assignment, and privileged
qualification must precede activation. No command/profile arguments are taken
from the invoking user; the only configuration path is fixed in config.py.
"""
import fcntl
import json
import os
from pathlib import Path
import pwd
import signal
import stat
import subprocess
import sys
import tempfile
import time

if __name__ == "__main__" and not globals().get("__bundle_verified__"):
    raise ValueError("owner must enter through the verified bootstrap")
sys.path.insert(0, str(Path(__file__).resolve().parent))
import bootstrap
import config as configuration
import custody
import deployment
import inventory
import login
import policy
from worker import write


def account(config):
    entry = pwd.getpwnam(config["user"])
    if (entry.pw_uid, entry.pw_gid) != (config["uid"], config["gid"]):
        raise ValueError("dedicated account identity mismatch")
    if Path(entry.pw_shell).name not in ("nologin", "false"):
        raise ValueError("development account must have no login shell")
    if set(os.getgrouplist(entry.pw_name, entry.pw_gid)) != {entry.pw_gid}:
        raise ValueError("development account has supplementary groups")
    shadow = [line.split(":") for line in Path("/etc/shadow").read_text().splitlines()
              if line.split(":", 1)[0] == entry.pw_name]
    if len(shadow) != 1 or not shadow[0][1].startswith(("!", "*")):
        raise ValueError("development account password must be locked")


def bus_identity():
    # This launcher is admitted only in PID 1's mount namespace. It then binds
    # this exact socket into a private root-owned /run/dbus; no user stand-in.
    directory = Path("/run/dbus").stat()
    info = Path("/run/dbus/system_bus_socket").lstat()
    bus_uid = pwd.getpwnam("dbus").pw_uid
    if (directory.st_uid not in (0, bus_uid) or directory.st_mode & 0o022
            or not stat.S_ISSOCK(info.st_mode) or info.st_uid not in (0, bus_uid)):
        raise ValueError("system bus path ownership/type is not admitted")
    return {"filesystem": info.st_dev, "inode": info.st_ino, "uid": info.st_uid,
            "directory_uid": directory.st_uid, "directory_mode": stat.S_IMODE(directory.st_mode)}


def preflight(config, api):
    if any(os.isatty(fd) for fd in (0, 1, 2)) or api.session(os.getpid()) is not None:
        raise ValueError("root owner must be outside every login and terminal")
    if any(name.startswith(("XDG_", "SOPHIA_")) or name in
           ("DISPLAY", "WAYLAND_DISPLAY", "DBUS_SYSTEM_BUS_ADDRESS", "DBUS_SESSION_BUS_ADDRESS")
           for name in os.environ):
        raise ValueError("service inherited desktop or bus environment")
    for name in ("mnt", "user", "net", "pid", "cgroup"):
        if os.readlink(f"/proc/self/ns/{name}") != os.readlink(f"/proc/1/ns/{name}"):
            raise ValueError("root service must start in the host namespaces")
    try:
        descriptor = os.open("/dev/tty", os.O_RDONLY | os.O_NOCTTY | os.O_CLOEXEC)
    except OSError as error:
        if error.errno not in (6, 19):  # ENXIO / ENODEV: no controlling tty
            raise
    else:
        os.close(descriptor)
        raise ValueError("root owner has a controlling terminal")
    account(config)
    if Path("/etc/pam.d/sophia-development").read_text() != deployment.pam_service(config):
        raise ValueError("dedicated PAM stack differs from the fixed stack")
    seat = config["seat"].encode()
    if api.number("sd_seat_can_tty", seat) != 0:
        raise ValueError("development seat must be non-VT")
    # sd_seat_get_active has two output pointers; never use UID display.
    import ctypes as C
    session, uid = C.c_void_p(), C.c_uint()
    fn = api.lib.sd_seat_get_active
    fn.argtypes, fn.restype = [C.c_char_p, C.POINTER(C.c_void_p), C.POINTER(C.c_uint)], C.c_int
    try:
        status = fn(seat, C.byref(session), C.byref(uid))
        if status != -61:  # ENODATA means no active session; every other state refuses.
            raise ValueError(f"development seat is occupied or unavailable: {status}")
    finally:
        api.libc.free(session)
    return {"inventory": inventory.discover(config), "bus": bus_identity(),
            "policy_files": policy.verify(config)}


def observed_owned_login(api, config, child):
    # The child has not been reaped. Even an exited child's PID cannot be
    # reused until finish(). This covers registration before worker receipt.
    try:
        return login.attest(api, config, child.process.pid)
    except (ValueError, OSError):
        return None


def terminate_owned_login(api, config, child, observed):
    current = observed_owned_login(api, config, child)
    if current is None or observed is None or current != observed:
        return {"status": "not_attested"}
    cleanup = subprocess.run([config["tools"]["loginctl"]["path"], "terminate-session",
                              current["session"]], stdin=subprocess.DEVNULL,
                             capture_output=True, timeout=5,
                             env={"PATH": "/usr/bin:/bin", "LANG": "C.UTF-8"})
    return {"status": "requested", "session": current["session"], "exit": cleanup.returncode}


def cleanup_worker(api, config, child, observed, result):
    # Worker cleanup may use 2 s TERM grace + 5 s reap wait. Allow a margin for
    # PAM/receipt before escalating through logind, which may kill the worker.
    child.send(signal.SIGTERM)
    if not child.ready(8):
        try:
            result["login_cleanup"] = terminate_owned_login(api, config, child, observed)
        except BaseException as error:
            result["login_cleanup_error"] = str(error)
    # No second TERM grace: the 8 s above already elapsed if still alive.
    result["cleanup_exit"] = child.finish(terminate=True, grace=0)


def run_once(config, api, initial):
    root = Path(config["output_root"])
    configuration.trusted(root)
    lock_path = root / (config["seat"] + ".lock")
    lock_fd = os.open(lock_path, os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600)
    try:
        info = os.fstat(lock_fd)
        if not stat.S_ISREG(info.st_mode) or info.st_uid != 0 or stat.S_IMODE(info.st_mode) != 0o600:
            raise ValueError("untrusted seat lock")
        fcntl.flock(lock_fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
        # Recheck seat occupancy after the per-seat exclusion is held.
        current = preflight(config, api)
        if current != initial:
            raise ValueError("host admission changed before launch")
        run = Path(tempfile.mkdtemp(prefix=config["seat"] + "-", dir=root))
        # Reserve cleanup time inside the service's independent hard timeout.
        outer_deadline = time.monotonic() + config["outer_seconds"]
        # 8 s worker grace + 5 s loginctl + 5 s final reap, plus receipt margin.
        deadline = outer_deadline - 20
        write(run / "declaration.json", {"config": config, **current, "deadline": deadline})
        result = {"schema": 1, "status": "failed", "run": str(run), "deadline": deadline,
                  "outer_deadline": outer_deadline}
        child = None
        observed = None
        try:
            argv = bootstrap.command(config, "worker", str(run))
            with (run / "worker.log").open("xb") as output:
                with custody.defer_term():
                    child = custody.Child(custody.command(config, argv), stdin=subprocess.DEVNULL,
                                          stdout=output, stderr=output,
                                          env={"PATH": "/usr/bin:/bin", "LANG": "C.UTF-8"})
                while not child.ready(0.1):
                    # Keep an owned identity while the worker is alive. This
                    # does not depend on worker publishing login.json first.
                    found = observed_owned_login(api, config, child)
                    if found:
                        if observed and found["session"] != observed["session"]:
                            raise ValueError("worker changed registered session")
                        observed = found
                    if time.monotonic() >= deadline:
                        raise TimeoutError("outer development deadline expired")
                observed = observed_owned_login(api, config, child) or observed
                result["worker_exit"] = child.finish()
                child = None
            worker = json.loads((run / "worker-result.json").read_text())
            result["worker"] = worker
            if result["worker_exit"] != 0 or worker.get("status") != "completed":
                raise ValueError("development worker did not complete")
            result["status"] = "completed"
        except BaseException as error:
            result["reason"] = str(error)
        finally:
            signal.pthread_sigmask(signal.SIG_BLOCK, {signal.SIGTERM})
            if child is not None:
                observed = observed_owned_login(api, config, child) or observed
                try:
                    cleanup_worker(api, config, child, observed, result)
                except BaseException as error:
                    result["cleanup_error"] = str(error)
            result["owned_login"] = observed
            # Never issue a session operation after reaping its owned leader.
            # Normal exit closes PAM; forced exit closes its lifetime pipe.
            try:
                preflight(config, api)
            except (ValueError, OSError) as error:
                result["status"], result["postflight_error"] = "failed", str(error)
            write(run / "result.json", result)
        print(json.dumps(result, sort_keys=True))
        return 0 if result["status"] == "completed" else 2
    finally:
        os.close(lock_fd)


def main():
    if sys.argv[1:] not in (["--check"], ["--run"]):
        raise ValueError("usage: fixed root service owner.py --check|--run")
    os.umask(0o077)
    config = configuration.load()
    api = login.Login(config["tools"]["libelogind"]["path"])
    admission = preflight(config, api)
    if sys.argv[1] == "--check":
        print(json.dumps({"schema": 1, "status": "prepared", **admission,
                          "login_created": False, "devices_opened": 0}, sort_keys=True))
        return 0
    return run_once(config, api, admission)


if __name__ == "__main__":
    signal.signal(signal.SIGTERM, lambda *_: (_ for _ in ()).throw(InterruptedError("owner TERM")))
    try:
        sys.exit(main())
    except (ValueError, OSError, InterruptedError) as error:
        print(json.dumps({"status": "refused", "reason": str(error)}), file=sys.stderr)
        sys.exit(2)
