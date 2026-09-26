#!/usr/bin/env python3
"""Independent experimental control client; no live Sophia endpoint exists yet.

The stdlib-only decoder intentionally does not import generated Sophia code.
See docs/sophia-control-v1.md for authority, settlement, and retry semantics.
"""

import argparse
import array
import json
import os
import socket
import struct
import sys
import time

HEADER = struct.Struct("<4sHHQII")
WELCOME = struct.Struct("<HHQQQQIHHIII")
OWNERS = {1: "policy", 2: "session"}
COMPLETIONS = {1: "policy-commit", 2: "session-settlement"}
OUTCOMES = dict(enumerate(("committed", "completed", "unchanged", "rejected",
                          "stale", "denied", "unavailable", "overloaded",
                          "timed-out", "indeterminate"), 1))
ERRORS = {1: "malformed", 2: "sequence", 3: "revision", 4: "features"}
SESSION_NAMES = {"logout", "reload-profile", "restart-wm"}
# Catalog capacity per revision: 256 WM actions plus that revision's session operations.
MAX_COMMANDS = {1: 258, 2: 259}


class ProtocolViolation(ValueError):
    pass


def require(condition, detail):
    if not condition:
        raise ProtocolViolation(detail)


def padded_text(raw, length, name=False):
    require(0 <= length <= len(raw) and not any(raw[length:]), "string padding/length")
    try:
        text = raw[:length].decode("utf-8")
    except UnicodeDecodeError as error:
        raise ProtocolViolation("invalid UTF-8") from error
    require(all(ord(c) >= 32 and not 127 <= ord(c) <= 159 for c in text), "control character")
    if name:
        require(bool(text) and text == text.strip(" "), "empty/untrimmed name")
        require(all(c.isascii() and (c.isalnum() or c in " -_.") for c in text), "name grammar")
    return text


def name_bytes(name):
    raw = name.encode("utf-8")
    require(len(raw) <= 128, "name too long")
    padded_text(raw, len(raw), name=True)
    return len(raw), raw.ljust(128, b"\0")


def header_values(raw):
    require(len(raw) == HEADER.size, "truncated header")
    magic, version, kind, request_id, length, reserved = HEADER.unpack(raw)
    require(magic == b"SOPH" and version == 1 and reserved == 0, "invalid envelope")
    require(kind in range(128, 135) and length <= 65536, "kind/size")
    require(kind not in (128, 129) or request_id == 0, "handshake ID")
    require(kind not in (130, 131, 132, 133) or request_id != 0, "zero request ID")
    return kind, request_id, length


def decode_frame(raw):
    kind, request_id, length = header_values(raw[:HEADER.size])
    require(len(raw) == HEADER.size + length, "truncated/trailing frame")
    payload = raw[HEADER.size:]
    expected = {128: 12, 129: WELCOME.size, 130: 0, 132: 140, 133: 268, 134: 4}
    if kind in expected:
        require(length == expected[kind], "payload size")
    result = {"kind": kind, "request_id": request_id}
    if kind == 128:
        minimum, maximum, features = struct.unpack("<HHQ", payload)
        require(0 < minimum <= maximum, "revision range")
        result.update(minimum_revision=minimum, maximum_revision=maximum, required_features=features)
    elif kind == 129:
        revision, reserved, low, high, connection, features, limit, count, names, command, frame, idle = WELCOME.unpack(payload)
        require(revision in MAX_COMMANDS and reserved == 0 and features == 0, "welcome negotiation")
        require((low or high) and connection > 0, "welcome identity")
        require((limit, count, names) == (65536, MAX_COMMANDS[revision], 128), "welcome capacities")
        require(0 < command <= 10000 and 0 < frame <= 2000 and 0 < idle <= 60000, "welcome deadlines")
        result.update(session_id=[low, high], connection_id=connection,
                      command_timeout_ms=command, frame_timeout_ms=frame, idle_timeout_ms=idle)
    elif kind == 131:
        require(length >= 12, "catalog prefix")
        generation, count, reserved = struct.unpack_from("<QHH", payload)
        require(generation > 0 and count <= max(MAX_COMMANDS.values()) and reserved == 0 and length == 12 + 136 * count, "catalog shape")
        entries = []
        previous = None
        for offset in range(12, length, 136):
            owner, completion, name_len, reserved = struct.unpack_from("<HHHH", payload, offset)
            require(owner in OWNERS and completion == owner and reserved == 0, "catalog entry")
            name = padded_text(payload[offset + 8:offset + 136], name_len, name=True)
            require(owner != 2 or name in SESSION_NAMES, "session command")
            key = (owner, name)
            require(previous is None or previous < key, "catalog order/duplicate")
            entries.append({"owner": OWNERS[owner], "completion": COMPLETIONS[completion], "name": name})
            previous = key
        require(sum(entry["owner"] == "policy" for entry in entries) <= 256, "policy count")
        result.update(catalog_generation=generation, commands=entries)
    elif kind == 132:
        generation, owner, name_len = struct.unpack_from("<QHH", payload)
        require(generation > 0 and owner in OWNERS, "invoke identity")
        result.update(catalog_generation=generation, owner=OWNERS[owner],
                      name=padded_text(payload[12:], name_len, name=True))
    elif kind == 133:
        generation, outcome, detail_len = struct.unpack_from("<QHH", payload)
        require(generation > 0 and outcome in OUTCOMES, "outcome identity/code")
        result.update(catalog_generation=generation, outcome=OUTCOMES[outcome],
                      detail=padded_text(payload[12:], detail_len))
    elif kind == 134:
        error, reserved = struct.unpack("<HH", payload)
        require(error in ERRORS and reserved == 0, "protocol error code/reserved")
        result["error"] = ERRORS[error]
    return result


def encode_request(kind, request_id, generation=None, owner=None, name=None):
    if kind == 128:
        payload = struct.pack("<HHQ", 1, 2, 0)
    elif kind == 130:
        payload = b""
    elif kind == 132:
        length, raw = name_bytes(name)
        payload = struct.pack("<QHH", generation, owner, length) + raw
    else:
        raise ProtocolViolation("not a client request")
    frame = HEADER.pack(b"SOPH", 1, kind, request_id, len(payload), 0) + payload
    decode_frame(frame)
    return frame


def recv_exact(stream, length, deadline):
    result = bytearray()
    while len(result) < length:
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise TimeoutError("control receive deadline")
        stream.settimeout(remaining)
        data, ancillary, flags, _ = stream.recvmsg(length - len(result), socket.CMSG_SPACE(4 * 16), socket.MSG_CMSG_CLOEXEC)
        # Never leak descriptors from a malformed peer, including on truncation.
        for level, kind, value in ancillary:
            if level == socket.SOL_SOCKET and kind == socket.SCM_RIGHTS:
                descriptors = array.array("i")
                descriptors.frombytes(value[:len(value) - len(value) % descriptors.itemsize])
                for descriptor in descriptors:
                    os.close(descriptor)
        require(not ancillary and not flags & (socket.MSG_CTRUNC | socket.MSG_TRUNC), "unexpected ancillary data")
        if not data:
            raise EOFError("control peer disconnected")
        result.extend(data)
    return bytes(result)


def receive_frame(stream, wait_seconds, frame_seconds):
    # The response wait and incomplete-frame deadlines are separate; trickle
    # traffic cannot renew the latter.
    first = recv_exact(stream, 1, time.monotonic() + wait_seconds)
    deadline = time.monotonic() + frame_seconds
    header = first + recv_exact(stream, HEADER.size - 1, deadline)
    _, _, length = header_values(header)
    return decode_frame(header + recv_exact(stream, length, deadline))


class Client:
    """Synchronous one-outstanding-request client. Never reconnects or replays."""

    def __init__(self, stream):
        self.stream = stream
        self.next_id = 1
        self.catalog = None
        self.closed = False
        self.invocation_pending = False
        self.frame_seconds = 2.0
        self.wait_seconds = 2.0
        self.welcome = self._exchange(encode_request(128, 0), 129, 0)
        self.frame_seconds = self.welcome["frame_timeout_ms"] / 1000
        self.wait_seconds = self.welcome["command_timeout_ms"] / 1000 + self.frame_seconds

    def _exchange(self, frame, expected_kind, request_id):
        require(not self.closed, "connection closed after previous failure")
        try:
            self.stream.settimeout(self.frame_seconds)
            self.stream.sendall(frame)
            reply = receive_frame(self.stream, self.wait_seconds, self.frame_seconds)
            if reply["kind"] == 134:
                raise ProtocolViolation("peer protocol error: " + reply["error"])
            require((reply["kind"], reply["request_id"]) == (expected_kind, request_id), "reply correlation/direction")
            return reply
        except (OSError, EOFError, ValueError):
            self.closed = True
            self.stream.close()
            raise

    def _request(self, kind, **fields):
        require(self.next_id < 2**64, "request ID exhausted; reconnect and rediscover")
        request_id = self.next_id
        frame = encode_request(kind, request_id, **fields)
        self.next_id += 1
        if kind == 132:
            # Even a failed send may have written enough for the peer to act.
            self.invocation_pending = True
        return self._exchange(frame, kind + 1, request_id)

    def commands(self):
        self.catalog = self._request(130)
        return self.catalog

    def invoke(self, owner, name):
        catalog = self.catalog if self.catalog is not None else self.commands()
        require(any(entry["owner"] == owner and entry["name"] == name for entry in catalog["commands"]), "command absent from authorized catalog")
        reply = self._request(132, generation=catalog["catalog_generation"],
                              owner={value: key for key, value in OWNERS.items()}[owner], name=name)
        allowed_success = {"committed"} if owner == "policy" else {"completed"}
        if owner == "session" and name == "reload-profile":
            allowed_success.add("unchanged")
        try:
            require(reply["catalog_generation"] == catalog["catalog_generation"], "outcome generation")
            require(reply["outcome"] not in {"committed", "completed", "unchanged"} - allowed_success, "wrong owner settlement")
        except ProtocolViolation:
            self.closed = True
            self.stream.close()
            raise
        if owner == "session" or reply["outcome"] in {"stale", "denied", "indeterminate"}:
            self.catalog = None
        self.invocation_pending = False
        return reply


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--socket", default=os.environ.get("SOPHIA_CONTROL_SOCKET"))
    parser.add_argument("owner", choices=("commands", "policy", "session"))
    parser.add_argument("name", nargs="?")
    args = parser.parse_args()
    if not args.socket or not os.path.isabs(args.socket) or "\0" in args.socket:
        parser.error("an absolute --socket or SOPHIA_CONTROL_SOCKET is required")
    if (args.owner == "commands") != (args.name is None):
        parser.error("commands takes no name; policy/session require one")
    client = None
    try:
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as stream:
            stream.settimeout(2)
            stream.connect(args.socket)
            _, uid, _ = struct.unpack("3i", stream.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, 12))
            require(uid == os.geteuid(), "unexpected server user")
            client = Client(stream)
            result = client.commands()
            if args.owner != "commands":
                result = client.invoke(args.owner, args.name)
            print(json.dumps(result, sort_keys=True))
            return 0 if result.get("outcome", "completed") in {"committed", "completed", "unchanged"} else 1
    except (OSError, EOFError, ValueError) as error:
        uncertain = client is not None and client.invocation_pending
        print(json.dumps({"error": str(error), "outcome": "unknown" if uncertain else "not-invoked"}), file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
