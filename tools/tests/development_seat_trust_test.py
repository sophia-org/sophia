"""File-policy and ELF controls; fixtures are never executed as root."""
import hashlib
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

from development_seat_login_test import fixture_config


SOURCE = Path(__file__).resolve().parents[1] / "development_seat"
sys.path.insert(0, str(SOURCE))
import audit
import deployment
import elf
import policy


class LoaderPaths(unittest.TestCase):
    def test_real_elf_search_paths_are_read_without_running_the_program(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "inert.c"
            source.write_text("int main(void) { return 17; }\n")
            for rpath, expected in (("$ORIGIN/libraries:/usr/lib", [str(root / "libraries"), "/usr/lib"]),
                                    ("relative", None), ("$LIB", None), ("/usr/lib:", None)):
                binary = root / "program"
                subprocess.run(["cc", str(source), "-o", str(binary), "-Wl,-rpath," + rpath],
                               check=True, capture_output=True, timeout=30)
                if expected is None:
                    with self.assertRaisesRegex(ValueError, "relative ELF"):
                        elf.loader_paths(binary)
                else:
                    paths = elf.loader_paths(binary)
                    self.assertEqual(paths[-2:], expected)
                    self.assertTrue(Path(paths[0]).is_absolute())
            binary.write_bytes(b"#!/bin/sh\nexit 0\n")
            with self.assertRaisesRegex(ValueError, "ELF64"):
                elf.loader_paths(binary)


class FileTrust(unittest.TestCase):
    def test_missing_or_rewritten_profile_refuses_before_exec(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            executable, profile = root / "executable", root / "profile.kdl"
            executable.write_bytes(b"inert")
            profile.write_bytes(b"frozen profile")
            pin = lambda path: {"path": str(path), "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}
            config = {"bundle": str(root), "tools": {key: pin(executable) for key in
                      ("sophia", "drop", "python", "nested_bubblewrap")}, "files": [pin(profile)]}
            audit.verify_files(config)
            profile.write_bytes(b"changed profile")
            with self.assertRaisesRegex(ValueError, "bundle file hash"):
                audit.verify_files(config)
            profile.unlink()
            with self.assertRaisesRegex(ValueError, "bundle directory inventory"):
                audit.verify_files(config)

    def test_pam_service_uses_only_the_two_pinned_absolute_modules(self):
        config = fixture_config()
        text = deployment.pam_service(config)
        self.assertEqual([line for line in text.splitlines() if not line.startswith("#")],
                         ["account required /usr/bin/pam_permit", "session required /usr/bin/pam_elogind"])
        config["tools"]["pam_elogind"]["path"] = "/usr/lib/module with space"
        with self.assertRaisesRegex(ValueError, "whitespace"):
            deployment.pam_service(config)

    def test_unreviewed_or_earlier_host_policy_refuses(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            etc, share = root / "etc", root / "share"
            etc.mkdir(); share.mkdir()
            deny = etc / "00-000-sophia-development.rules"
            deny.write_text(policy.deny_rule("sophia-dev"))
            config = {"user": "sophia-dev", "files": [{"path": str(deny), "sha256": policy.digest(deny)}]}
            # No host policy is read; ownership is separately enforced by
            # config.trusted. This exercises ordering and complete pinning.
            with patch.object(policy, "DIRECTORIES", (etc, share)), patch.object(policy, "DENY_PATH", deny), \
                 patch.object(policy, "trusted"):
                self.assertEqual(policy.verify(config), {str(deny): policy.digest(deny)})
                later = share / "later.rules"
                later.write_text("unreviewed")
                with self.assertRaisesRegex(ValueError, "changed"):
                    policy.verify(config)
                later.unlink()
                duplicate = share / deny.name
                duplicate.write_text(deny.read_text())
                with self.assertRaisesRegex(ValueError, "ambiguous"):
                    policy.verify(config)
                duplicate.unlink()
                (share / "00-000-a.rules").write_text("earlier grant")
                with self.assertRaisesRegex(ValueError, "not first"):
                    policy.verify(config)


if __name__ == "__main__":
    unittest.main()
