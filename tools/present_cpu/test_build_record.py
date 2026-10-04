import argparse
import contextlib
import io
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from build_record import build, digest


class Receipt(unittest.TestCase):
    def run_build(self, mutation):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            target = root / "target"
            (target / "release/examples").mkdir(parents=True)
            args = argparse.Namespace(out=root / "receipt", target=target, wrapper=None)
            sophia = target / "release/sophia"
            environment = {"configs": {}, "environment": {"RUSTFLAGS": "original"}}
            changed = {"configs": {}, "environment": {"RUSTFLAGS": "changed"}}

            def cargo(command, **kwargs):
                if "sophia-cli" in command:
                    sophia.write_bytes(b"built-cli")
                else:
                    (target / "release/examples/present_cpu_workload").write_bytes(b"built-client")
                    if mutation == "binary":
                        sophia.write_bytes(b"concurrent-writer")
                return subprocess.CompletedProcess(command, 0)

            readings = [json.dumps(environment), json.dumps(changed if mutation == "config" else environment)]
            with patch("build_record.source_identity", return_value={"head": "frozen"}), \
                    patch("build_record.subprocess.check_output", side_effect=readings), \
                    patch("build_record.subprocess.run", side_effect=cargo), contextlib.redirect_stdout(io.StringIO()):
                if mutation:
                    with self.assertRaisesRegex(RuntimeError, "configuration or binaries changed"):
                        build(args)
                else:
                    build(args)
            receipt = json.loads((args.out / "build.json").read_text())
            self.assertEqual(receipt["status"], "FAILED" if mutation else "PASS")
            self.assertEqual(receipt["final_hashes"]["sophia"], digest(sophia))
            if mutation == "binary":
                self.assertNotEqual(receipt["files"]["sophia"]["sha256"], receipt["final_hashes"]["sophia"])

    def test_artifacts_and_environment_stay_bound_to_each_build(self):
        for mutation in (None, "binary", "config"):
            with self.subTest(mutation=mutation):
                self.run_build(mutation)


if __name__ == "__main__":
    unittest.main()
