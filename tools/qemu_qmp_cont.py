#!/usr/bin/env python3
"""Resume a QEMU guest started paused (-S) over QMP."""

import sys

# The client is borrowed from a sibling script; leave no cache in tools/.
sys.dont_write_bytecode = True
from qemu_qmp_type import QmpClient  # noqa: E402


def main():
    if len(sys.argv) != 2:
        raise SystemExit("usage: qemu_qmp_cont.py QMP_SOCKET")
    with QmpClient(sys.argv[1]) as client:
        client.execute("cont")


if __name__ == "__main__":
    main()
