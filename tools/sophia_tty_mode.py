#!/usr/bin/env python3
"""Get or set Linux virtual-terminal display and keyboard modes."""

from __future__ import annotations

import array
import fcntl
import os
import sys

KDSETMODE = 0x4B3A
KDGETMODE = 0x4B3B
KD_TEXT = 0
KD_GRAPHICS = 1
KDGKBMODE = 0x4B44
KDSKBMODE = 0x4B45
K_OFF = 4


def open_tty(tty_path: str):
    """Reuse standard input only when it is exactly the target terminal.

    Releasing a logind session controller resets the VT node to root:tty 0620,
    but the login's open descriptor keeps its access. Any other target,
    including the default /dev/tty, is opened by path.
    """
    try:
        inherited = os.isatty(0) and os.ttyname(0) == tty_path
    except OSError:
        inherited = False
    if inherited:
        return os.fdopen(os.dup(0), "rb+", buffering=0)
    return open(tty_path, "rb+", buffering=0)


def main() -> int:
    if len(sys.argv) != 2:
        raise SystemExit(
            "usage: sophia_tty_mode.py get|text|graphics|MODE|"
            "get-keyboard|keyboard-off|keyboard-MODE"
        )
    action = sys.argv[1]
    tty_path = os.environ.get("SOPHIA_SESSION_TTY", "/dev/tty")
    with open_tty(tty_path) as tty:
        tty_fd = tty.fileno()
        if action == "get-keyboard":
            value = array.array("i", [0])
            fcntl.ioctl(tty_fd, KDGKBMODE, value, True)
            print(value[0])
            return 0
        if action == "keyboard-off":
            fcntl.ioctl(tty_fd, KDSKBMODE, K_OFF)
            return 0
        if action.startswith("keyboard-"):
            fcntl.ioctl(tty_fd, KDSKBMODE, int(action.removeprefix("keyboard-"), 10))
            return 0
        if action == "get":
            value = array.array("i", [0])
            fcntl.ioctl(tty_fd, KDGETMODE, value, True)
            print(value[0])
            return 0
        if action == "text":
            mode = KD_TEXT
        elif action == "graphics":
            mode = KD_GRAPHICS
        else:
            mode = int(action, 10)
        fcntl.ioctl(tty_fd, KDSETMODE, mode)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
