import base64
import gzip
import hashlib
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

from log_transport import BEGIN, END, unpack


class LogTransport(unittest.TestCase):
    def export(self, raw):
        return (f"sophia_qemu_cpu_log schema=1 transport=tmpfs "
                f"export=gzip_base64_after_measurement bytes={len(raw)} "
                f"sha256={hashlib.sha256(raw).hexdigest()} tmpfs_used_kib=4 export_start_uptime=10.00\n" +
                BEGIN + base64.encodebytes(gzip.compress(raw)).decode() + END +
                "sophia_qemu_cpu_log schema=1 export_end_uptime=11.00 status=0\n")

    def test_exact_bytes_and_full_diagnostics_survive(self):
        raw = b"sophia_runtime_fatal failure_code=example\n\x1b[31mtracing\x1b[0m\n"
        text = self.export(raw)
        decoded, actual, record = unpack(text)
        self.assertEqual(actual, raw)
        self.assertIn(raw.decode(), decoded)
        self.assertEqual(record["bytes"], len(raw))
        self.assertEqual(record["export_seconds"], 1.0)

    def test_missing_corrupt_duplicate_and_oversized_exports_fail(self):
        valid = self.export(b"x" * 100)
        for text in ("", BEGIN + "?!" + END, BEGIN + "AAAA" + END, valid + valid, valid,
                     valid.replace("bytes=100", "bytes=99")):
            with self.subTest(text=text[:30]):
                with self.assertRaises(ValueError):
                    unpack(text, limit=99)

    def test_streaming_export_keeps_result_and_reports_compressor_failure(self):
        script = Path(__file__).with_name("export_guest_log.sh")
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "session.log").write_text("full tracing\n" * 100)
            (root / "workload.json").write_text('{"status":"complete"}')
            for name in ("interrupts-before", "interrupts-after"):
                (root / name).write_text("interrupts\n")
            result = subprocess.run(["sh", str(script), directory], capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertLess(result.stdout.index("sophia_present_cpu_result"), result.stdout.index(BEGIN))
            self.assertEqual(unpack(result.stdout)[1], (root / "session.log").read_bytes())
            self.assertFalse((root / "session.log.gz").exists())
            # Simulates a failed compressor after it wrote a partial stream. The
            # encoder succeeds, but export must still fail and keep the result.
            (root / "gzip").write_text("#!/bin/sh\nprintf broken\nexit 1\n")
            (root / "gzip").chmod(0o755)
            env = dict(os.environ, PATH=directory + os.pathsep + os.environ["PATH"])
            result = subprocess.run(["sh", str(script), directory], env=env, capture_output=True, text=True)
            self.assertEqual(result.returncode, 1)
            self.assertIn('sophia_present_cpu_result {"status":"complete"}', result.stdout)
            self.assertIn("bytes=", result.stdout)
            self.assertIn("sha256=", result.stdout)
            self.assertIn("status=failed reason=export", result.stdout)
            self.assertNotIn(END, result.stdout)
            with self.assertRaises(ValueError):
                unpack(result.stdout)


if __name__ == "__main__":
    unittest.main()
