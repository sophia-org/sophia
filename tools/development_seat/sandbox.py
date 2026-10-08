"""Pure command construction for the one fixed development Session."""
from pathlib import Path


NAMESPACES = ("mnt", "pid", "ipc", "uts", "net", "user")


def environment(config, login):
    return {"PATH": "/usr/bin:/bin", "HOME": "/home/development", "LANG": "C.UTF-8",
            "USER": config["user"], "LOGNAME": config["user"],
            "XDG_RUNTIME_DIR": f"/run/user/{config['uid']}",
            "XDG_SESSION_ID": login["session"], "XDG_SEAT": config["seat"],
            "XDG_SESSION_TYPE": "wayland", "LIBSEAT_BACKEND": "logind"}


def session_command(config):
    return [config["tools"]["sophia"]["path"], "session", "--native-scanout", "--no-input",
            f"--development-seat={config['seat']}", f"--max-runtime-ms={config['runtime_ms']}",
            f"--desktop-profile={config['profile']}",
            f"--bubblewrap={config['tools']['nested_bubblewrap']['path']}"]


def command(config, run, login, inventory, admission_fd, *, nix_store):
    bundle = Path(config["bundle"])
    # No unshare-all: host user and network namespaces are deliberate, needed
    # for kernel/root udev credentials. The post-drop scopes/filter close IPC.
    result = [config["tools"]["bubblewrap"]["path"], "--unshare-pid", "--unshare-ipc",
              "--unshare-uts", "--die-with-parent", "--new-session", "--clearenv",
              "--cap-drop", "ALL", "--cap-add", "CAP_SETUID", "--cap-add", "CAP_SETGID",
              "--cap-add", "CAP_SETPCAP", "--tmpfs", "/", "--ro-bind", "/usr", "/usr",
              "--symlink", "usr/bin", "/bin", "--symlink", "usr/lib", "/lib",
              "--symlink", "usr/lib", "/lib64", "--dir", "/etc",
              "--ro-bind", "/etc/ld.so.cache", "/etc/ld.so.cache",
              "--ro-bind", str(run / "passwd"), "/etc/passwd",
              "--ro-bind", str(run / "group"), "/etc/group"]
    if nix_store:
        result += ["--ro-bind", "/nix/store", "/nix/store"]
    result += ["--ro-bind", str(bundle), str(bundle), "--ro-bind", "/sys", "/sys",
               "--proc", "/proc", "--dev", "/dev", "--dir", "/dev/dri"]
    for node in inventory["nodes"]:
        result += ["--dev-bind", node["node"], node["node"]]
    result += ["--tmpfs", "/tmp", "--tmpfs", "/run", "--dir", "/run/dbus",
               "--ro-bind", "/run/dbus/system_bus_socket", "/run/dbus/system_bus_socket",
               "--ro-bind", "/run/udev", "/run/udev",
               "--ro-bind", "/run/systemd", "/run/systemd",
               "--dir", "/run/user", "--bind", str(run / "runtime"), f"/run/user/{config['uid']}",
               "--tmpfs", "/home", "--bind", str(run / "home"), "/home/development",
               "--bind", str(run / "artifacts"), "/results",
               "--ro-bind", str(run / "inside.json"), "/run/inside.json", "--chdir", "/home/development"]
    for name, value in environment(config, login).items():
        result += ["--setenv", name, value]
    # Root drops credentials before loading the auditor, then the auditor
    # closes the admission pipe before exec. PAM descriptors never get here.
    result += ["--", config["tools"]["drop"]["path"], str(config["uid"]), str(config["gid"]),
               "--", config["tools"]["python"]["path"], "-I", "-B", str(bundle / "audit.py"),
               str(admission_fd)]
    return result
