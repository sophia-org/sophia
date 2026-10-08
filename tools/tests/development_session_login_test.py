"""Device-free controls for secondary-login admission; no PAM session is opened."""
import copy
import ctypes
import errno
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import development_session_login as admission


def identity():
    return {"pid": 321, "start_ticks": 1234, "uid": 1000, "euid": 1000,
            "cgroup": "0::/88\n", "namespaces": {"pid": "a", "user": "b"}}


class FakeLogin:
    library = {"path": "/usr/lib/libelogind.so.0", "sha256": "a" * 64}

    def __init__(self):
        self.calls = []
        self.values = {
            "sd_pid_get_session": "88", "sd_session_get_seat": "seat-sophia-dev",
            "sd_session_get_type": "wayland", "sd_session_get_class": "user",
            "sd_session_get_state": "active", "sd_session_get_service": "sophia-development",
            "sd_session_get_tty": None, "sd_session_get_vt": None,
            "sd_session_get_uid": 1000, "sd_session_is_active": 1,
            "sd_session_is_remote": 0, "sd_seat_can_tty": 0,
        }

    def string(self, name, first, *args, **kwargs):
        self.calls.append((name, first, args, kwargs))
        result = self.values[name]
        if isinstance(result, Exception):
            raise result
        return result

    scalar = string


class LoginAdmission(unittest.TestCase):
    def setUp(self):
        self.login = FakeLogin()
        self.patch = patch.object(admission, "process_identity", side_effect=identity)
        self.patch.start()
        self.addCleanup(self.patch.stop)

    def attest(self, **environment):
        return admission.attest(self.login, "seat-sophia-dev", "sophia-development", environment)

    def test_own_pid_only_and_two_complete_observations(self):
        row = self.attest(XDG_SESSION_ID="88", XDG_SEAT="seat-sophia-dev")
        self.assertEqual(row["session"], "88")
        requests = [call for call in self.login.calls if call[0] == "sd_pid_get_session"]
        self.assertEqual(len(requests), 2)
        self.assertTrue(all(call[1] == 321 and call[2] == (ctypes.c_int,) for call in requests))
        self.assertFalse(any("display" in call[0] for call in self.login.calls))
        for name in self.login.values:
            self.assertEqual(sum(call[0] == name for call in self.login.calls), 2)

    def test_missing_pid_session_never_uses_environment_as_authority(self):
        self.login.values["sd_pid_get_session"] = admission.Refused("no own session")
        with self.assertRaisesRegex(admission.Refused, "no own session"):
            self.attest(XDG_SESSION_ID="88", XDG_SEAT="seat-sophia-dev")
        self.assertEqual(len(self.login.calls), 1)

    def test_every_authority_field_refuses_independently(self):
        changes = {"sd_session_get_seat": "seat0", "sd_session_get_type": "tty",
                   "sd_session_get_class": "greeter", "sd_session_get_state": "online",
                   "sd_session_get_service": "sshd", "sd_session_get_tty": "tty7",
                   "sd_session_get_vt": 7, "sd_session_get_uid": 1001,
                   "sd_session_is_active": 0, "sd_session_is_remote": 1,
                   "sd_seat_can_tty": 1}
        for key, value in changes.items():
            with self.subTest(key=key), self.assertRaises(admission.Refused):
                self.login = FakeLogin()
                self.login.values[key] = value
                self.attest()

    def test_root_and_mismatched_effective_uid_refuse(self):
        for uid, euid in [(0, 0), (1000, 0), (1000, 1001)]:
            with self.subTest(uid=uid, euid=euid), \
                    patch.object(admission, "process_identity", return_value=dict(identity(), uid=uid, euid=euid)), \
                    self.assertRaisesRegex(admission.Refused, "unprivileged"):
                self.attest()

    def test_login_and_process_changes_fail_closed(self):
        original = identity()
        for key, value in [("start_ticks", 9999), ("cgroup", "0::/99\n"), ("pid", 999)]:
            changed = dict(original, **{key: value})
            for sequence in ([original, changed], [original, original, changed, changed]):
                with self.subTest(key=key, sequence=sequence), \
                        patch.object(admission, "process_identity", side_effect=sequence), \
                        self.assertRaisesRegex(admission.Refused, "changed"):
                    self.attest()
        first = admission.observe(self.login)
        second = copy.deepcopy(first)
        second["session"] = "89"
        with patch.object(admission, "observe", side_effect=[first, second]), \
                self.assertRaisesRegex(admission.Refused, "between observations"):
            self.attest()

    def test_ambient_session_or_seat_or_vt_cannot_override_observation(self):
        for env in ({"XDG_SESSION_ID": "76"}, {"XDG_SEAT": "seat0"}, {"XDG_VTNR": "7"}):
            with self.subTest(env=env), self.assertRaisesRegex(admission.Refused, "inherited"):
                self.attest(**env)

    def test_seat0_and_unbounded_or_path_names_refuse(self):
        row = admission.observe(self.login)
        for seat in ("seat0", "", "../seat1", "seat" + "x" * 60):
            with self.subTest(seat=seat), self.assertRaisesRegex(admission.Refused, "non-seat0"):
                admission.validate(row, seat, "sophia-development")

    def test_absent_vt_and_zero_vt_both_mean_no_vt(self):
        for tty, vt in [(None, None), ("", 0)]:
            self.login.values.update(sd_session_get_tty=tty, sd_session_get_vt=vt)
            self.assertEqual(self.attest()["vt"], vt)

    def test_only_enodata_means_absent_not_unknown_or_permission_denied(self):
        self.assertFalse(admission.Login.checked("get_vt", -errno.ENODATA, absent=True))
        for rc in (-errno.ENXIO, -errno.EACCES, -errno.ENOENT, -errno.EIO):
            with self.subTest(rc=rc), self.assertRaises(admission.Refused):
                admission.Login.checked("get_vt", rc, absent=True)
        with self.assertRaises(admission.Refused):
            admission.Login.checked("get_seat", -errno.ENODATA)


class Receipt(unittest.TestCase):
    def test_refusal_is_recorded_and_existing_receipt_is_preserved(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "admission.json"
            argv = ["admission", "--library", "/absent.so", "--sha256", "0" * 64,
                    "--seat", "seat-sophia-dev", "--output", str(output)]
            with patch.object(sys, "argv", argv), patch("builtins.print"):
                self.assertEqual(admission.main(), 2)
                record = json.loads(output.read_text())
                self.assertEqual(record["status"], "refused")
                self.assertTrue(record["reason"])
                self.assertEqual(output.stat().st_mode & 0o777, 0o600)
                before = output.read_bytes()
                with self.assertRaises(FileExistsError):
                    admission.main()
                self.assertEqual(output.read_bytes(), before)

    def test_untrusted_library_is_refused_before_loading(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "library.so"
            path.write_bytes(b"not code")
            with patch.object(admission.ctypes, "CDLL") as load, \
                    self.assertRaisesRegex(admission.Refused, "root-owned"):
                admission.Login(path, "0" * 64)
            load.assert_not_called()


if __name__ == "__main__":
    unittest.main()
