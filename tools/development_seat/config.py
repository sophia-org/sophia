"""Fixed root-owned launch bundle; parsing never opens a login or a device."""
import hashlib
import json
import os
from pathlib import Path
import re
import stat


CONFIG = Path("/etc/sophia-development/launch.json")
TOOLS = {"python", "timeout", "bubblewrap", "nested_bubblewrap", "custody", "guard", "drop", "sophia",
         "libpam", "pam_permit", "pam_elogind", "libelogind", "libudev", "loginctl"}
FIELDS = {"schema", "user", "uid", "gid", "daily_uids", "seat", "pci", "runtime_ms",
          "outer_seconds", "bundle", "tools", "files", "profile", "pam_service",
          "output_root"}


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def absolute(value):
    if not isinstance(value, str) or "\0" in value:
        raise ValueError("path must be a string without NUL")
    path = Path(value)
    if not path.is_absolute() or str(path) != value or ".." in path.parts:
        raise ValueError("path must be absolute and normalized")
    return path


def trusted(path, regular=False):
    """Require every component to be root-owned, not writable by other UIDs.

    Symlinks are refused in configured paths. Use canonical paths in the fixed
    configuration; the mutable source checkout is never a privileged bundle.
    """
    path = absolute(str(path))
    for part in [*reversed(path.parents), path]:
        info = part.lstat()
        if info.st_uid != 0 or stat.S_ISLNK(info.st_mode) or info.st_mode & 0o022:
            raise ValueError(f"untrusted root bundle path: {part}")
    info = path.stat()
    if regular and (not stat.S_ISREG(info.st_mode) or info.st_mode & 0o6000):
        raise ValueError(f"expected ordinary file: {path}")
    return info


def integer(value, lower, upper, name):
    if type(value) is not int or not lower <= value <= upper:
        raise ValueError(f"invalid {name}")
    return value


def parse(value):
    if (not isinstance(value, dict) or set(value) != FIELDS or
            type(value["schema"]) is not int or value["schema"] != 1):
        raise ValueError("unknown development launch schema or fields")
    integer(value["uid"], 1, 2**31-1, "uid")
    integer(value["gid"], 1, 2**31-1, "gid")
    if not re.fullmatch(r"[a-z_][a-z0-9_-]{0,31}", value["user"]):
        raise ValueError("invalid dedicated user")
    daily = value["daily_uids"]
    if not isinstance(daily, list) or not daily or len(set(daily)) != len(daily):
        raise ValueError("daily uid exclusions required")
    for uid in daily:
        integer(uid, 1, 2**31-1, "daily uid")
    if value["uid"] in daily:
        raise ValueError("development account must differ from daily accounts")
    if not re.fullmatch(r"seat[A-Za-z0-9_-]{1,59}", value["seat"]) or value["seat"] == "seat0":
        raise ValueError("a secondary seat is required")
    if not re.fullmatch(r"[0-9a-f]{4}:[0-9a-f]{2}:[0-9a-f]{2}\.[0-7]", value["pci"]):
        raise ValueError("invalid PCI identity")
    runtime = integer(value["runtime_ms"], 1, 300000, "runtime bound")
    outer = integer(value["outer_seconds"], 1, 360, "outer bound")
    if outer * 1000 < runtime + 30000:
        raise ValueError("outer bound must include at least 30 seconds for login and cleanup")
    bundle = absolute(value["bundle"])
    if not str(bundle).startswith("/opt/sophia-development/"):
        raise ValueError("bundle must be staged under /opt/sophia-development")
    output = absolute(value["output_root"])
    if output != Path("/var/lib/sophia-development/runs"):
        raise ValueError("output root is fixed")
    if set(value["tools"]) != TOOLS:
        raise ValueError("all tool and host-library pins are required")
    for pin in [*value["tools"].values(), *value["files"]]:
        if not isinstance(pin, dict) or set(pin) != {"path", "sha256"}:
            raise ValueError("invalid file pin")
        absolute(pin["path"])
        if not re.fullmatch(r"[0-9a-f]{64}", pin["sha256"]):
            raise ValueError("invalid file hash")
    paths = [pin["path"] for pin in value["files"]]
    if len(set(paths)) != len(paths):
        raise ValueError("duplicate bundle file")
    required = {str(bundle / name) for name in
                ("owner.py", "worker.py", "config.py", "login.py", "pam.py", "sandbox.py", "audit.py",
                 "custody.py", "inventory.py", "policy.py", "elf.py", "deployment.py")}
    if not required.issubset(paths):
        raise ValueError("launcher source pins incomplete")
    profile = absolute(value["profile"])
    if not profile.is_relative_to(bundle) or str(profile) not in paths:
        raise ValueError("profile must be a pinned bundle file")
    if value["pam_service"] != "sophia-development":
        raise ValueError("dedicated PAM service required")
    if "/etc/pam.d/sophia-development" not in paths:
        raise ValueError("PAM service pin required")
    if "/etc/polkit-1/rules.d/00-000-sophia-development.rules" not in paths:
        raise ValueError("dedicated account polkit restriction pin required")
    return value


def verify(value):
    from elf import loader_paths
    parse(value)
    trusted(Path(value["bundle"]))
    for pin in [*value["tools"].values(), *value["files"]]:
        path = absolute(pin["path"])
        trusted(path, regular=True)
        if digest(path) != pin["sha256"]:
            raise ValueError(f"pin mismatch: {path}")
    for pin in value["tools"].values():
        for name in loader_paths(Path(pin["path"])):
            path = Path(name)
            # System interpreter aliases are normal, but each alias must be
            # root-owned and the canonical destination must be trusted too.
            for part in [*reversed(path.parents), path]:
                info = part.lstat()
                if info.st_uid != 0 or (not stat.S_ISLNK(info.st_mode) and info.st_mode & 0o022):
                    raise ValueError(f"untrusted ELF loader path: {part}")
            trusted(path.resolve(strict=True))


def load():
    if os.getuid() != 0 or os.geteuid() != 0:
        raise ValueError("launcher must run from its root-owned one-shot service")
    trusted(CONFIG, regular=True)
    value = parse(json.loads(CONFIG.read_text()))
    verify(value)
    return value
