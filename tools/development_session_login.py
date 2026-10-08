#!/usr/bin/env python3
"""Read-only own-process admission for a non-VT development login.

Uses the host's pinned sd-login implementation, never the UID's display session
or a caller-supplied PID. This does not create a session, grant devices, launch
Sophia, or replace confinement of a future launcher. Run it in the login that
will launch Sophia, before entering namespaces that hide the host PID/cgroup.
"""
import argparse
import ctypes
import errno
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import sys


class Refused(ValueError):
    pass


def process_identity():
    fields = Path("/proc/self/stat").read_text().rsplit(") ", 1)[1].split()
    return {"pid": os.getpid(), "start_ticks": int(fields[19]),
            "cgroup": Path("/proc/self/cgroup").read_text(),
            "uid": os.getuid(), "euid": os.geteuid(),
            "namespaces": {name: os.readlink(f"/proc/self/ns/{name}")
                           for name in ("pid", "user", "mnt", "cgroup")}}


def trusted_library(path, expected):
    if not path.is_absolute() or not re.fullmatch(r"[0-9a-f]{64}", expected):
        raise Refused("an absolute library path and SHA-256 are required")
    path = path.resolve(strict=True)
    for entry in (path, *path.parents):
        info = entry.stat()
        if info.st_uid != 0 or info.st_mode & 0o022:
            raise Refused("login library and its parents must be root-owned and not writable")
    if not stat.S_ISREG(path.stat().st_mode):
        raise Refused("login library must be a regular file")
    with path.open("rb") as stream:
        observed = hashlib.file_digest(stream, "sha256").hexdigest()
    if observed != expected:
        raise Refused("host login library changed")
    return path


class Login:
    """sd-login C ABI. All returned strings are freed by the host process libc."""
    def __init__(self, path, sha256):
        path = trusted_library(path, sha256)
        self.library = {"path": str(path), "sha256": sha256}
        self.lib = ctypes.CDLL(str(path))
        self.libc = ctypes.CDLL(None)
        self.libc.free.argtypes = [ctypes.c_void_p]
        self.libc.free.restype = None

    @staticmethod
    def checked(name, rc, absent=False):
        # ENODATA is the sd-login contract for an absent TTY/VT. Other errors
        # (including an unknown/disappearing session) must not look inputless.
        if absent and rc == -errno.ENODATA:
            return False
        if rc < 0:
            raise Refused(f"{name}: {os.strerror(-rc)}")
        return True

    def string(self, name, first, kind=ctypes.c_char_p, absent=False):
        fn = getattr(self.lib, name)
        fn.argtypes = [kind, ctypes.POINTER(ctypes.c_void_p)]
        fn.restype = ctypes.c_int
        value = ctypes.c_void_p()
        try:
            if not self.checked(name, fn(first, ctypes.byref(value)), absent):
                return None
            if not value.value:
                raise Refused(f"{name}: empty result")
            return ctypes.string_at(value).decode("ascii")
        finally:
            self.libc.free(value)

    def scalar(self, name, first, output=False, absent=False):
        fn = getattr(self.lib, name)
        fn.restype = ctypes.c_int
        value = ctypes.c_uint()
        if output:
            fn.argtypes = [ctypes.c_char_p, ctypes.POINTER(ctypes.c_uint)]
            rc = fn(first, ctypes.byref(value))
        else:
            fn.argtypes = [ctypes.c_char_p]
            rc = fn(first)
        if not self.checked(name, rc, absent):
            return None
        return value.value if output else rc


def observe(login):
    before = process_identity()
    session = login.string("sd_pid_get_session", before["pid"], ctypes.c_int)
    if not re.fullmatch(r"[A-Za-z0-9_-]{1,64}", session):
        raise Refused("invalid session identity")
    key = session.encode("ascii")
    row = {"process": before, "session": session, "library": login.library}
    for field in ("seat", "type", "class", "state", "service"):
        row[field] = login.string("sd_session_get_" + field, key)
    row["tty"] = login.string("sd_session_get_tty", key, absent=True)
    row["vt"] = login.scalar("sd_session_get_vt", key, output=True, absent=True)
    row["uid"] = login.scalar("sd_session_get_uid", key, output=True)
    for field in ("active", "remote"):
        row[field] = login.scalar("sd_session_is_" + field, key)
    row["seat_has_vts"] = login.scalar("sd_seat_can_tty", row["seat"].encode("ascii"))
    if process_identity() != before:
        raise Refused("process identity changed during login query")
    return row


def validate(row, seat, service):
    if not re.fullmatch(r"seat[A-Za-z0-9_-]{1,59}", seat) or seat == "seat0":
        raise Refused("development seat must be an explicit non-seat0 seat")
    if not re.fullmatch(r"[A-Za-z0-9_-]{1,64}", service):
        raise Refused("invalid login service name")
    process = row["process"]
    if process["uid"] == 0 or process["uid"] != process["euid"] or row["uid"] != process["uid"]:
        raise Refused("development login must belong to the unprivileged caller")
    if (row["seat"] != seat or row["service"] != service or row["active"] != 1
            or row["remote"] != 0 or row["state"] != "active" or row["class"] != "user"
            or row["type"] not in ("wayland", "x11")):
        raise Refused("caller is not the expected active local graphical login")
    if row["seat_has_vts"] != 0 or row["tty"] not in (None, "") or row["vt"] not in (None, 0):
        raise Refused("development login must have no TTY or VT")
    return row


def attest(login, seat, service, environment):
    first = validate(observe(login), seat, service)
    second = validate(observe(login), seat, service)
    if first != second:
        raise Refused("login changed between observations")
    for name, expected in (("XDG_SESSION_ID", second["session"]), ("XDG_SEAT", seat)):
        if name in environment and environment[name] != expected:
            raise Refused(f"inherited {name} disagrees with own-process login")
    if environment.get("XDG_VTNR", "") not in ("", "0"):
        raise Refused("inherited XDG_VTNR names a VT")
    return second


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--library", type=Path, required=True)
    parser.add_argument("--sha256", required=True)
    parser.add_argument("--seat", required=True)
    parser.add_argument("--service", default="sophia-development")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    # Reserve the receipt before observation. An existing run is never changed.
    fd = os.open(args.output, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_CLOEXEC, 0o600)
    with os.fdopen(fd, "w") as stream:
        receipt = {"schema": 1, "status": "refused", "expected_seat": args.seat,
                   "expected_service": args.service, "process": process_identity(),
                   "scope": "read-only login observation; no device or launch authority"}
        try:
            login = Login(args.library, args.sha256)
            receipt["login"] = attest(login, args.seat, args.service, os.environ)
            receipt["status"] = "admitted"
        except (ValueError, OSError, AttributeError) as error:
            receipt["reason"] = str(error)
        json.dump(receipt, stream, indent=2, sort_keys=True)
        stream.write("\n")
    print(json.dumps(receipt, sort_keys=True))
    return 0 if receipt["status"] == "admitted" else 2


if __name__ == "__main__":
    sys.exit(main())
