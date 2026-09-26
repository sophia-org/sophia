"""Offline codec/client conformance, deliberately not a session security test."""

import importlib.util
import os
from pathlib import Path
import socket
import struct
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[3]
MODULE = importlib.util.spec_from_file_location("control", ROOT / "bindings/python/sophia_control_v1.py")
wire = importlib.util.module_from_spec(MODULE)
MODULE.loader.exec_module(wire)


def corpus(name):
    return {label: bytes.fromhex(hex_frame)
            for line in (ROOT / "protocol/golden" / name).read_text().splitlines()
            if line and not line.startswith("#")
            for label, hex_frame in [line.split()]}


VALID = corpus("sophia-control-v1.frames")


def frame(kind, request_id, payload):
    return wire.HEADER.pack(b"SOPH", 1, kind, request_id, len(payload), 0) + payload


def changed(raw, offset, value, fmt="H"):
    result = bytearray(raw)
    struct.pack_into("<" + fmt, result, offset, value)
    return bytes(result)


def outcome(code, request_id=2, generation=1):
    return frame(133, request_id, struct.pack("<QHH", generation, code, 0) + bytes(256))


def entry(owner, name):
    length, raw = wire.name_bytes(name)
    return struct.pack("<HHHH", owner, owner, length, 0) + raw


class FragmentedStream:
    """Prewritten peer transcript; it implements no authorization or dispatch."""

    def __init__(self, data, fragment=1, ancillary=(), flags=0):
        self.data = data
        self.fragment = fragment
        self.ancillary = ancillary
        self.flags = flags
        self.sent = []
        self.closed = False

    def settimeout(self, value):
        self.timeout = value

    def sendall(self, data):
        self.sent.append(data)

    def recvmsg(self, length, _space, _flags):
        count = min(length, self.fragment)
        result, self.data = self.data[:count], self.data[count:]
        ancillary, self.ancillary = self.ancillary, ()
        return result, ancillary, self.flags, None

    def close(self):
        self.closed = True


class CodecTests(unittest.TestCase):
    def test_schema_symbolic_assignments_match_independent_client(self):
        expected = {
            "owner": wire.OWNERS, "completion": wire.COMPLETIONS,
            "outcome": wire.OUTCOMES, "error": wire.ERRORS,
            "message": dict(enumerate(("ClientHello", "ServerWelcome", "CommandsRequest",
                                       "CommandsReply", "Invoke", "CommandOutcome", "ProtocolError"), 128)),
        }
        wanted = sorted((family, name, str(value)) for family, entries in expected.items()
                        for value, name in entries.items())
        path = ROOT / "protocol/golden/sophia-control-v1.values"
        actual = sorted(tuple(line.split()) for line in path.read_text().splitlines()
                        if line and not line.startswith("#"))
        self.assertEqual(actual, wanted)

    def test_schema_generated_messages_decode(self):
        self.assertEqual(len(VALID), 7)
        for name, raw in VALID.items():
            with self.subTest(name=name):
                wire.decode_frame(raw)

    def test_independent_request_encoding_matches_schema(self):
        self.assertEqual(wire.encode_request(128, 0), VALID["ClientHello"])
        self.assertEqual(wire.encode_request(130, 1), VALID["CommandsRequest"])
        self.assertEqual(wire.encode_request(132, 1, generation=1, owner=1, name="focus-next"), VALID["Invoke"])

    def test_generated_malformed_frames(self):
        for name, raw in corpus("sophia-control-v1-malformed.frames").items():
            with self.subTest(name=name), self.assertRaises(wire.ProtocolViolation):
                wire.decode_frame(raw)

    def test_fragmentation_at_every_boundary(self):
        for raw in VALID.values():
            for size in (1, 2, 3, 7, 23, 24, 25, 65560):
                self.assertEqual(wire.receive_frame(FragmentedStream(raw, size), 2, 2), wire.decode_frame(raw))

    def test_empty_and_full_catalog(self):
        self.assertEqual(wire.decode_frame(frame(131, 1, struct.pack("<QHH", 1, 0, 0)))["commands"], [])
        entries = [entry(1, f"action-{i:03d}") for i in range(256)]
        entries += [entry(2, "reload-profile"), entry(2, "restart-wm")]
        raw = frame(131, 1, struct.pack("<QHH", 1, len(entries), 0) + b"".join(entries))
        self.assertEqual(len(raw) - 24, 35100)
        self.assertEqual(len(wire.decode_frame(raw)["commands"]), 258)

    def test_revision_two_welcome_capacity_and_logout(self):
        def welcome(revision, count):
            return frame(129, 0, wire.WELCOME.pack(revision, 0, 1, 2, 1, 0, 65536, count, 128, 10000, 2000, 60000))
        self.assertEqual(wire.decode_frame(welcome(1, 258))["kind"], 129)
        self.assertEqual(wire.decode_frame(welcome(2, 259))["kind"], 129)
        for revision, count in ((2, 258), (1, 259), (3, 260), (0, 258)):
            with self.subTest(revision=revision, count=count), self.assertRaises(wire.ProtocolViolation):
                wire.decode_frame(welcome(revision, count))
        entries = [entry(1, f"action-{i:03d}") for i in range(256)]
        entries += [entry(2, "logout"), entry(2, "reload-profile"), entry(2, "restart-wm")]
        raw = frame(131, 1, struct.pack("<QHH", 1, len(entries), 0) + b"".join(entries))
        self.assertEqual(len(wire.decode_frame(raw)["commands"]), 259)
        unknown = frame(131, 1, struct.pack("<QHH", 1, 1, 0) + entry(2, "shutdown"))
        with self.assertRaises(wire.ProtocolViolation):
            wire.decode_frame(unknown)

    def test_strict_payload_fields(self):
        cases = [
            changed(VALID["ClientHello"], 24, 0),
            changed(VALID["ClientHello"], 24, 3),
            changed(VALID["ServerWelcome"], 24, 3),
            changed(VALID["ServerWelcome"], 26, 1),
            changed(VALID["ServerWelcome"], 52, 1, "Q"),
            changed(VALID["ServerWelcome"], 60, 65535, "I"),
            changed(VALID["ServerWelcome"], 68, 10001, "I"),
            changed(VALID["ServerWelcome"], 72, 0, "I"),
            changed(VALID["CommandsReply"], 24, 0, "Q"),
            changed(VALID["CommandsReply"], 32, 259),
            changed(VALID["CommandsReply"], 34, 1),
            changed(VALID["CommandsReply"], 36, 3),
            changed(VALID["CommandsReply"], 38, 2),
            changed(VALID["CommandsReply"], 40, 129),
            changed(VALID["CommandsReply"], 42, 1),
            changed(VALID["Invoke"], 8, 0, "Q"),
            changed(VALID["Invoke"], 24, 0, "Q"),
            changed(VALID["Invoke"], 32, 3),
            changed(VALID["Invoke"], 34, 0),
            changed(VALID["Invoke"], 36, 255, "B"),
            changed(VALID["Invoke"], 36, 32, "B"),
            changed(VALID["Invoke"], 46, 1, "B"),
            changed(VALID["CommandOutcome"], 32, 11),
            changed(VALID["CommandOutcome"], 34, 257),
            changed(VALID["CommandOutcome"], 36, 1, "B"),
            changed(VALID["ProtocolError"], 24, 5),
            changed(VALID["ProtocolError"], 26, 1),
        ]
        for index, raw in enumerate(cases):
            with self.subTest(case=index), self.assertRaises(wire.ProtocolViolation):
                wire.decode_frame(raw)

    def test_catalog_duplicate_order_and_owner_limits(self):
        cases = [
            [entry(1, "a"), entry(1, "a")],
            [entry(1, "b"), entry(1, "a")],
            [entry(2, "launch")],
            [entry(1, f"a-{i:03d}") for i in range(257)],
        ]
        for entries in cases:
            with self.assertRaises(wire.ProtocolViolation):
                wire.decode_frame(frame(131, 1, struct.pack("<QHH", 1, len(entries), 0) + b"".join(entries)))

    def test_all_outcomes_and_bounded_utf8_diagnostics(self):
        for code, label in wire.OUTCOMES.items():
            self.assertEqual(wire.decode_frame(outcome(code))["outcome"], label)
        for detail in ("", "rejected: réglage", "x" * 256):
            raw = detail.encode()
            decoded = wire.decode_frame(frame(133, 2, struct.pack("<QHH", 1, 4, len(raw)) + raw.ljust(256, b"\0")))
            self.assertEqual(decoded["detail"], detail)
        for raw in (b"\xff", b"\x00", b"\x1b", b"\xc2\x85"):
            with self.assertRaises(wire.ProtocolViolation):
                wire.decode_frame(frame(133, 2, struct.pack("<QHH", 1, 4, len(raw)) + raw.ljust(256, b"\0")))

    def test_trickle_deadline_does_not_reset(self):
        with patch.object(wire.time, "monotonic", side_effect=range(100)):
            with self.assertRaises(TimeoutError):
                wire.receive_frame(FragmentedStream(VALID["ClientHello"]), 2, 2)

    def test_oversize_rejected_before_body_read(self):
        raw = changed(VALID["ClientHello"], 16, 65537, "I")
        stream = FragmentedStream(raw)
        with self.assertRaises(wire.ProtocolViolation):
            wire.receive_frame(stream, 2, 2)
        self.assertEqual(stream.data, raw[24:])

    def test_ancillary_descriptors_closed(self):
        read_fd, write_fd = os.pipe()
        try:
            stream = FragmentedStream(b"S", ancillary=[(socket.SOL_SOCKET, socket.SCM_RIGHTS, struct.pack("i", read_fd))])
            with self.assertRaises(wire.ProtocolViolation):
                wire.receive_frame(stream, 2, 2)
            with self.assertRaises(OSError):
                os.fstat(read_fd)
        finally:
            os.close(write_fd)
        with self.assertRaises(wire.ProtocolViolation):
            wire.receive_frame(FragmentedStream(b"S", flags=socket.MSG_CTRUNC), 2, 2)


class ClientTests(unittest.TestCase):
    def peer(self, reply):
        stream = FragmentedStream(VALID["ServerWelcome"] + VALID["CommandsReply"] + reply)
        return wire.Client(stream), stream

    def test_reusable_connection_monotonic_ids(self):
        client, stream = self.peer(outcome(1) + changed(VALID["CommandsReply"], 8, 3, "Q"))
        self.assertEqual(client.invoke("policy", "focus-next")["outcome"], "committed")
        self.assertFalse(client.invocation_pending)
        client.commands()
        self.assertEqual([wire.decode_frame(raw)["request_id"] for raw in stream.sent], [0, 1, 2, 3])

    def test_negotiation_errors_close_connection(self):
        for code in (3, 4):
            stream = FragmentedStream(frame(134, 0, struct.pack("<HH", code, 0)))
            with self.assertRaises(wire.ProtocolViolation):
                wire.Client(stream)
            self.assertTrue(stream.closed)
            self.assertEqual(len(stream.sent), 1)

    def test_stale_requires_rediscovery_and_no_automatic_replay(self):
        client, stream = self.peer(outcome(5))
        self.assertEqual(client.invoke("policy", "focus-next")["outcome"], "stale")
        self.assertIsNone(client.catalog)
        self.assertEqual(len(stream.sent), 3)

    def test_failure_outcomes_are_not_success_or_retried(self):
        for code in range(4, 11):
            client, stream = self.peer(outcome(code))
            self.assertEqual(client.invoke("policy", "focus-next")["outcome"], wire.OUTCOMES[code])
            self.assertEqual(len(stream.sent), 3)

    def test_wrong_ids_generation_direction_and_settlement(self):
        for reply in (outcome(1, 1), outcome(1, 0), outcome(1, 3), outcome(1, generation=2),
                      outcome(2), outcome(3), VALID["Invoke"]):
            client, stream = self.peer(reply)
            with self.assertRaises(wire.ProtocolViolation):
                client.invoke("policy", "focus-next")
            self.assertTrue(stream.closed)

    def test_disconnect_never_replays_partial_or_missing_reply(self):
        for reply in (b"", outcome(1)[:23], outcome(1)[:-1]):
            client, stream = self.peer(reply)
            with self.assertRaises(EOFError):
                client.invoke("policy", "focus-next")
            self.assertTrue(client.invocation_pending)
            with self.assertRaises(wire.ProtocolViolation):
                client.invoke("policy", "focus-next")
            self.assertEqual(len(stream.sent), 3)

    def test_missing_command_not_sent_and_ids_do_not_wrap(self):
        client, stream = self.peer(b"")
        client.commands()
        with self.assertRaises(wire.ProtocolViolation):
            client.invoke("policy", "absent")
        self.assertFalse(client.invocation_pending)
        client.next_id = 2**64
        with self.assertRaises(wire.ProtocolViolation):
            client.commands()
        self.assertEqual(len(stream.sent), 2)

    def test_session_result_echoes_old_catalog_then_invalidates_cache(self):
        catalog = frame(131, 1, struct.pack("<QHH", 7, 2, 0) + entry(2, "reload-profile") + entry(2, "restart-wm"))
        for name, code in (("reload-profile", 2), ("reload-profile", 3), ("reload-profile", 4), ("restart-wm", 2)):
            stream = FragmentedStream(VALID["ServerWelcome"] + catalog + outcome(code, generation=7))
            client = wire.Client(stream)
            self.assertEqual(client.invoke("session", name)["catalog_generation"], 7)
            self.assertIsNone(client.catalog)


if __name__ == "__main__":
    unittest.main()
