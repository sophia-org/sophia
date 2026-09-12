"""Offline checks for the independent client, not server conformance results."""
import json
from pathlib import Path
import socket
import struct
import time
import unittest
from unittest.mock import patch

import report
import xtest_cases as cases


class PackingClient:
    def __init__(self, order='<'):
        self.order = order

    def pack(self, fmt, *values):
        return struct.pack(self.order + fmt, *values)


class XTestClientTests(unittest.TestCase):
    def test_setup_frame_keeps_missing_name_and_data_distinct(self):
        for order, prefix, lengths in (
                ('<', b'l\0', bytes.fromhex('0b000000030002000000')),
                ('>', b'B\0', bytes.fromhex('000b0000000300020000'))):
            with self.subTest(order=order):
                self.assertEqual(cases.setup_request(order, b'ABC', b'xy'),
                                 prefix + lengths + b'ABC\0xy\0\0')
                named = cases.setup_request(order, b'ABC', b'')
                data_only = cases.setup_request(order, b'', b'xy')
                self.assertEqual(struct.unpack_from(order + 'HH', named, 6), (3, 0))
                self.assertEqual(struct.unpack_from(order + 'HH', data_only, 6), (0, 2))
                self.assertNotEqual(named, cases.setup_request(order, b'', b''))
                self.assertNotEqual(data_only, cases.setup_request(order, b'', b''))

    def test_setup_refusal_negative_catches_anonymous_fallback(self):
        class Response:
            def __init__(self, data):
                self.data = data

            def settimeout(self, timeout):
                assert timeout > 0

            def recv(self, size):
                # Fragmentation exercises the same bounded reader as the wire.
                part, self.data = self.data[:min(size, 3)], self.data[min(size, 3):]
                return part
        for order in ('<', '>'):
            for status in (1, 2):
                with self.subTest(order=order, status=status):
                    response = Response(bytes([status, 0]) + struct.pack(order + 'HHH', 11, 0, 0))
                    with self.assertRaisesRegex(AssertionError, 'fell through'):
                        cases.expect_setup_refused(response, order, time.monotonic() + 1)
            refused = Response(bytes([0, 3]) + struct.pack(order + 'HHH', 11, 0, 1) + b'bad\0')
            cases.expect_setup_refused(refused, order, time.monotonic() + 1)

    def test_setup_refusal_does_not_disclose_server_reason(self):
        class Response:
            def settimeout(self, timeout):
                pass

            def recv(self, size):
                return bytes([0, 99]) + struct.pack('<HHH', 11, 0, 1)
        with self.assertRaisesRegex(AssertionError, '^truncated setup-refusal reason$'):
            cases.expect_setup_refused(Response(), '<', time.monotonic() + 1)

    def test_fake_input_21_external_layout_little(self):
        # Fixed bytes from the 36-byte XTEST 2.1 request field table, excluding
        # the four-byte request header. No Sophia encoder participates.
        body = cases.fake_body(PackingClient(), 2, 38, 0x01020304,
                               0x11223344, -2, 258, 255)
        self.assertEqual(body, bytes.fromhex(
            '02260000 04030201 44332211 0000000000000000 feff0201 '
            '00000000000000ff'))
        self.assertEqual(len(body) + 4, 36)

    def test_fake_input_21_external_layout_big(self):
        body = cases.fake_body(PackingClient('>'), 6, 1, 0xffffffff,
                               0x11223344, -32768, 32767, 165)
        self.assertEqual(body, bytes.fromhex(
            '06010000 ffffffff 11223344 0000000000000000 80007fff '
            '00000000000000a5'))

    def test_fake_input_keeps_card32_delay_and_padding_separate(self):
        for order in ('<', '>'):
            body = cases.fake_body(PackingClient(order), 6, delay=0xffffffff, padding=255)
            self.assertEqual(struct.unpack_from(order + 'I', body, 4)[0], 0xffffffff)
            self.assertEqual(body[31], 255)
            self.assertEqual(body[24:31], bytes(7))

    def test_fake_input_uses_discovered_major_and_minor_two(self):
        class Sender(PackingClient):
            def send(self, opcode, body, detail):
                self.sent = opcode, body, detail
                return 731
        c = Sender()
        self.assertEqual(cases.fake(c, 199, 3, detail=38), 731)
        self.assertEqual(c.sent[0], 199)
        self.assertEqual(c.sent[2], 2)
        self.assertEqual(c.sent[1][:2], bytes((3, 38)))

    def test_credentials_are_explicit_and_denial_not_uid_based(self):
        context = {'socket':'/not-connected', 'order':'<', 'deadline':123,
                   'auth_name':b'PRIVATE-RUN', 'auth_data':b'owned-credential'}
        with patch.object(cases, 'Client') as constructor:
            cases.client(context)
            constructor.assert_called_once_with('/not-connected', '<', 123,
                auth_name=b'PRIVATE-RUN', auth_data=b'owned-credential')
            constructor.reset_mock()
            cases.client(context, denied=True)
            constructor.assert_called_once_with('/not-connected', '<', 123,
                auth_name=b'', auth_data=b'')

    def test_early_reply_negative_bites(self):
        # Both endpoints are owned by this test; no display connection.
        a, b = socket.socketpair()
        try:
            observer = type('Observer', (), {'sock':a, 'remaining':lambda _: .1})()
            b.sendall(b'early reply')
            with self.assertRaisesRegex(AssertionError, 'completed before'):
                cases.no_reply_yet(observer, .03)
        finally:
            a.close()
            b.close()

    def test_quiet_socket_wait_is_bounded(self):
        a, b = socket.socketpair()
        try:
            observer = type('Observer', (), {'sock':a, 'remaining':lambda _: .1})()
            started = time.monotonic()
            cases.no_reply_yet(observer, .01)
            self.assertLess(time.monotonic() - started, .5)
        finally:
            a.close()
            b.close()

    def test_key_observer_rejects_send_event_substitute(self):
        class Observer:
            root = 11

            def event(self, kind, predicate):
                event = bytearray(32)
                event[0], event[1] = kind | 128, 38
                struct.pack_into('<II', event, 8, 11, 22)
                assert predicate(event)
                return event

            def u32(self, data, offset):
                return struct.unpack_from('<I', data, offset)[0]
        with self.assertRaisesRegex(AssertionError, 'SendEvent'):
            cases.key_event(Observer(), 2, 22)

    def test_manifest_has_exact_mandatory_implementations(self):
        manifest = json.loads(Path(__file__).with_name('xtest_manifest.json').read_text())
        required = report.expected(manifest)
        self.assertEqual({name for name, _ in required}, set(cases.CASES))
        self.assertEqual(len(required), 2 * len(cases.CASES))
        self.assertTrue(all(case['mandatory'] for case in manifest['cases']))
        disabled = [case for case in manifest['cases'] if case['fixture'] == 'disabled']
        self.assertEqual([case['id'] for case in disabled], ['xtest_disabled'])

    def test_unexecuted_server_cases_cannot_pass_via_offline_tests(self):
        manifest = json.loads(Path(__file__).with_name('xtest_manifest.json').read_text())
        result = report.evaluate(manifest, [])
        self.assertEqual(result['status'], 'FAIL')
        self.assertEqual(result['executed'], 0)
        self.assertEqual(len(result['failures']), len(cases.CASES) * 2)


if __name__ == '__main__':
    unittest.main()
