"""Host-libc login inspection for the privileged owner/worker, before isolation.

Sophia independently authenticates its own session through the system bus. This
module is not used in the Nix-linked Session and never selects a UID display.
"""
import ctypes
import errno
import os
from pathlib import Path
import re


class Login:
    def __init__(self, library):
        self.lib = ctypes.CDLL(library)
        self.libc = ctypes.CDLL(None)
        self.libc.free.argtypes = [ctypes.c_void_p]
        self.libc.free.restype = None

    def string(self, name, key, pid=False, absent=False):
        fn = getattr(self.lib, name)
        fn.argtypes = [ctypes.c_int if pid else ctypes.c_char_p,
                       ctypes.POINTER(ctypes.c_void_p)]
        fn.restype = ctypes.c_int
        value = ctypes.c_void_p()
        try:
            status = fn(key, ctypes.byref(value))
            if absent and status == -errno.ENODATA:
                return None
            if status < 0 or not value.value:
                raise ValueError(f"{name}: status {status}")
            return ctypes.string_at(value).decode("ascii")
        finally:
            self.libc.free(value)

    def number(self, name, key, output=False, absent=False):
        fn = getattr(self.lib, name)
        fn.restype = ctypes.c_int
        value = ctypes.c_uint()
        if output:
            fn.argtypes = [ctypes.c_char_p, ctypes.POINTER(ctypes.c_uint)]
            status = fn(key, ctypes.byref(value))
        else:
            fn.argtypes = [ctypes.c_char_p]
            status = fn(key)
        if absent and status == -errno.ENODATA:
            return None
        if status < 0:
            raise ValueError(f"{name}: status {status}")
        return value.value if output else status

    def session(self, pid):
        return self.string("sd_pid_get_session", pid, pid=True, absent=True)

    def observe(self, pid):
        before = identity(pid)
        session = self.session(pid)
        if session is None or not re.fullmatch(r"[A-Za-z0-9_-]{1,64}", session):
            raise ValueError("worker has no valid registered session")
        key = session.encode("ascii")
        row = {"process": before, "session": session}
        for name in ("seat", "type", "class", "state", "service"):
            row[name] = self.string("sd_session_get_" + name, key)
        row["uid"] = self.number("sd_session_get_uid", key, output=True)
        row["tty"] = self.string("sd_session_get_tty", key, absent=True)
        row["vt"] = self.number("sd_session_get_vt", key, output=True, absent=True)
        for name in ("active", "remote"):
            row[name] = self.number("sd_session_is_" + name, key)
        row["can_tty"] = self.number("sd_seat_can_tty", row["seat"].encode("ascii"))
        if identity(pid) != before:
            raise ValueError("worker identity changed during login observation")
        return row


def identity(pid):
    root = Path(f"/proc/{pid}")
    fields = (root / "stat").read_text().rsplit(") ", 1)[1].split()
    return {"pid": pid, "start_ticks": int(fields[19]),
            "cgroup": (root / "cgroup").read_text()}


def validate(row, config):
    expected = {"seat": config["seat"], "uid": config["uid"],
                "service": config["pam_service"], "class": "user", "type": "wayland",
                "state": "active", "active": 1, "remote": 0, "can_tty": 0}
    if any(row.get(key) != value for key, value in expected.items()):
        raise ValueError("registered login differs from the dedicated non-VT seat")
    if row["tty"] not in (None, "") or row["vt"] not in (None, 0):
        raise ValueError("development login must have no TTY or VT")
    return row


def attest(api, config, pid=None):
    pid = os.getpid() if pid is None else pid
    first = validate(api.observe(pid), config)
    second = validate(api.observe(pid), config)
    if first != second:
        raise ValueError("login changed between observations")
    return second
