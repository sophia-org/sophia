"""The stand-in's service pass against real peers (qemu_lock_provider_service_control.c):
each way a peer ends the connection is recorded as itself, and the verifier
recognizes exactly the closures as teardowns, never an invalid reply."""
from pathlib import Path
import os
import subprocess
import sys
import tempfile
import unittest

from qemu_lock_provider_verifier_test import evidence, render

TOOLS = Path(__file__).resolve().parents[1]
ROOT = TOOLS.parent
SDK = ROOT / "vendor/c-desktop-sdk/source/src"
VERIFIER = TOOLS / "verify_qemu_session_lock_provider.py"
EXPECTED = {
    "orderly_eof": ("-3", "-3", None, "closed"),
    "closed_with_request_queued": ("-2", "-2", "32", "io"),
    "closed_with_request_unread": ("-2", "-2", "104", "io"),
    "malformed_reply": ("-1", "-1", None, None),
    "eof_mid_reply": ("-1", "-1", None, None),
}


class ServiceControlTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.directory = tempfile.TemporaryDirectory()
        binary = Path(cls.directory.name) / "control"
        sources = sorted(str(p) for p in (SDK / "lock_files").glob("*.c")) + \
            sorted(str(p) for p in (SDK / "nine_p").glob("*.c"))
        subprocess.run(["cc", "-std=c11", "-O2", "-Wall", "-Wextra", "-Werror", f"-I{SDK}", f"-I{TOOLS}",
                        str(TOOLS / "tests/qemu_lock_provider_service_control.c"), *sources,
                        "-o", str(binary)], check=True)
        result = subprocess.run([str(binary)], capture_output=True, text=True, check=True)
        cls.lines = {}
        for line in result.stdout.splitlines():
            fields = dict(word.split("=", 1) for word in line.split() if "=" in word)
            cls.lines[fields["mode"]] = (line, fields)

    @classmethod
    def tearDownClass(cls):
        cls.directory.cleanup()

    def verify(self, text):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "evidence.log"
            path.write_text(text)
            return subprocess.run([sys.executable, "-B", str(VERIFIER), str(path), "flood"],
                                  capture_output=True, text=True, check=False)

    def test_each_peer_ending_is_recorded_as_itself(self):
        self.assertEqual(set(self.lines), set(EXPECTED))
        for name, (rc, wire, errno, _) in EXPECTED.items():
            with self.subTest(control=name):
                _, fields = self.lines[name]
                self.assertEqual((fields["step"], fields["client"], fields["remote"], fields["refusal"]),
                                 ("service", "4", "0", "0"))
                self.assertEqual((fields["service_rc"], fields["wire"]), (rc, wire))
                if errno is not None:
                    self.assertEqual(fields["errno"], errno)

    def test_the_verifier_recognizes_only_the_closures(self):
        for name, (_, _, _, kind) in EXPECTED.items():
            with self.subTest(control=name):
                line, _ = self.lines[name]
                teardown = line.split(f"mode={name} ", 1)[1]
                result = self.verify(render(evidence("flood", teardown)))
                if kind is None:
                    self.assertEqual(result.returncode, 1, result.stdout)
                    self.assertIn("provider failed or reported malformed state", result.stderr)
                else:
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertIn(f"teardown_{kind}=1 ", result.stdout)


if __name__ == "__main__":
    unittest.main()
