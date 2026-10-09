#!/usr/bin/env python3
"""Press and release one key through QEMU's virtio keyboard at a fixed cadence.

Usage: qemu_qmp_cadence.py QMP_SOCKET KEY COUNT PERIOD_MS

Press I is sent at the host monotonic deadline START + I * PERIOD_MS over one
QMP connection, held 80 ms and released. One record per press follows its
release, with the host monotonic times of both events relative to START. These
times are cadence evidence only: QMP carries no sequence the guest can see, so
the guest's own records, not these, identify each delivered key.
"""

import json
import socket
import sys
import time

ALLOWED = {"f9"}
HOLD_S = 0.08


def fail(message):
    raise SystemExit(message)


def event(qcode, down):
    return {"type": "key", "data": {"down": down, "key": {"type": "qcode", "data": qcode}}}


def execute(stream, command, arguments=None):
    request = {"execute": command}
    if arguments is not None:
        request["arguments"] = arguments
    stream.write((json.dumps(request, separators=(",", ":")) + "\n").encode())
    stream.flush()
    while True:
        line = stream.readline()
        if not line:
            fail("QMP connection closed before a reply")
        message = json.loads(line)
        if "error" in message:
            fail(f"QMP command failed: {message['error']}")
        if "return" in message:
            return


def elapsed_ms(start):
    return (time.monotonic_ns() - start) // 1_000_000


def main():
    if len(sys.argv) != 5:
        fail("usage: qemu_qmp_cadence.py QMP_SOCKET KEY COUNT PERIOD_MS")
    key = sys.argv[2]
    if key not in ALLOWED:
        fail("key is not supported")
    try:
        count = int(sys.argv[3])
        period_ms = int(sys.argv[4])
    except ValueError:
        fail("count and period must be integers")
    if not 1 <= count <= 64 or not 250 <= period_ms <= 5000:
        fail("count must be from 1 through 64 and period from 250 through 5000 ms")
    connection = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    connection.settimeout(5)
    connection.connect(sys.argv[1])
    try:
        stream = connection.makefile("rwb", buffering=0)
        greeting = json.loads(stream.readline())
        if "QMP" not in greeting:
            fail("QMP greeting was missing")
        execute(stream, "qmp_capabilities")
        start = time.monotonic_ns()
        for index in range(count):
            deadline = start + index * period_ms * 1_000_000
            delay = deadline - time.monotonic_ns()
            if delay > 0:
                time.sleep(delay / 1e9)
            down = elapsed_ms(start)
            execute(stream, "input-send-event", {"events": [event(key, True)]})
            time.sleep(HOLD_S)
            execute(stream, "input-send-event", {"events": [event(key, False)]})
            up = elapsed_ms(start)
            print(
                f"sophia_qemu_unplug schema=1 status=key_cadence index={index} key={key} "
                f"down_ms={down} up_ms={up} late_ms={down - index * period_ms}",
                flush=True,
            )
    finally:
        connection.close()


if __name__ == "__main__":
    main()
