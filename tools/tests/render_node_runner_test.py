"""CPU controls for device selection, confinement and bounded test supervision."""
import argparse
import contextlib
import io
import json
import os
from pathlib import Path
import signal
import stat
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import run_render_node_test as runner


class DeviceSelection(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.sys = self.root / "class"
        self.dev = self.root / "dri"
        self.sys.mkdir()
        self.dev.mkdir()
        self.pci = "0000:16:00.0"

    def node(self, name="renderD129", pci=None):
        physical = self.root / (pci or self.pci)
        physical.mkdir(exist_ok=True)
        (physical / "vendor").write_text("0x1002\n")
        (physical / "device").write_text("0x164e\n")
        node = self.sys / name
        node.mkdir()
        (node / "device").symlink_to(physical, target_is_directory=True)
        (node / "dev").write_text("226:129\n")
        (self.dev / name).touch()

    def select(self, **overrides):
        info = dict(st_mode=stat.S_IFCHR | 0o660, st_rdev=os.makedev(226, 129),
                    st_ino=42, st_dev=12)
        info.update(overrides)
        with patch.object(runner, "device_stat", return_value=SimpleNamespace(**info)):
            return runner.select_device(self.pci, self.sys, self.dev)

    def test_exact_physical_gpu_selected_not_first_node(self):
        self.node("renderD128", "0000:03:00.0")
        self.node()
        device = self.select()
        self.assertEqual(device["node"], str(self.dev / "renderD129"))
        self.assertEqual(device["pci"], self.pci)
        self.assertEqual((device["major"], device["minor"]), (226, 129))

    def test_absent_and_ambiguous_are_refused(self):
        with self.assertRaisesRegex(ValueError, "found 0"):
            self.select()
        self.node()
        self.node("renderD130")
        with self.assertRaisesRegex(ValueError, "found 2"):
            self.select()

    def test_regular_file_or_wrong_device_is_refused(self):
        self.node()
        for change in [dict(st_mode=stat.S_IFREG | 0o660), dict(st_rdev=os.makedev(226, 0))]:
            with self.subTest(change=change), self.assertRaisesRegex(ValueError, "disagrees"):
                self.select(**change)

    def test_primary_node_is_not_a_candidate(self):
        self.node("card1")
        with self.assertRaisesRegex(ValueError, "found 0"):
            self.select()

    def test_no_path_or_partial_pci_selection(self):
        for value in ["renderD129", "16:00.0", "../card1", "0000:16:00.8", "0000:AB:00.0"]:
            with self.subTest(value=value), self.assertRaises(ValueError):
                runner.select_device(value, self.sys, self.dev)


class NamespaceCommand(unittest.TestCase):
    def test_only_one_device_and_no_ambient_state(self):
        command = runner.command("/bwrap", Path("/runner"), Path("/receipt"),
                                 {"node": "/dev/dri/renderD129"})
        mounts = [(command[i + 1], command[i + 2]) for i, x in enumerate(command)
                  if x == "--dev-bind"]
        self.assertEqual(mounts, [("/dev/dri/renderD129", "/dev/dri/renderD129")])
        readonly = [(command[i + 1], command[i + 2]) for i, x in enumerate(command)
                    if x == "--ro-bind"]
        self.assertNotIn(("/", "/"), readonly)
        writable = [(command[i + 1], command[i + 2]) for i, x in enumerate(command) if x == "--bind"]
        self.assertEqual(writable, [("/receipt/artifacts", "/results")])
        for item in ["--clearenv", "--unshare-all", "--unshare-user", "--die-with-parent", "--new-session",
                     "--disable-userns", "--assert-userns-disabled"]:
            self.assertIn(item, command)
        env = {command[i + 1]: command[i + 2] for i, x in enumerate(command) if x == "--setenv"}
        self.assertEqual(env["SOPHIA_TEST_RENDER_NODE"], "/dev/dri/renderD129")
        self.assertEqual(env["RUST_TEST_THREADS"], "1")
        self.assertFalse(set(env) & {"DISPLAY", "XAUTHORITY", "DBUS_SESSION_BUS_ADDRESS",
                                    "LD_PRELOAD", "SOPHIA_RUN_REAL_ATOMIC_SCANOUT_SMOKE"})
        for private in ["/", "/tmp", "/run", "/home"]:
            self.assertTrue(any(command[i:i + 2] == ["--tmpfs", private]
                                for i in range(len(command) - 1)))

    def test_same_namespace_cannot_enter(self):
        with self.assertRaisesRegex(ValueError, "private"):
            runner.audit({"host_namespaces": runner.namespace_ids()})

    def test_each_namespace_must_change(self):
        host = {name: "old-" + name for name in runner.NAMESPACES}
        observed = {name: "new-" + name for name in runner.NAMESPACES}
        runner.verify_namespaces(host, observed)
        for name in runner.NAMESPACES:
            with self.subTest(name=name), self.assertRaises(ValueError):
                runner.verify_namespaces(host, dict(observed, **{name: host[name]}))


class DeviceLocks(unittest.TestCase):
    def test_same_gpu_serialized_and_different_gpu_independent(self):
        with tempfile.TemporaryDirectory() as home, patch.object(runner.Path, "home", return_value=Path(home)):
            first = runner.acquire_lock("0000:16:00.0")
            try:
                with self.assertRaises(BlockingIOError):
                    runner.acquire_lock("0000:16:00.0")
                other = runner.acquire_lock("0000:03:00.0")
                os.close(other)
            finally:
                os.close(first)
            os.close(runner.acquire_lock("0000:16:00.0"))

    def test_lock_symlink_is_not_followed(self):
        with tempfile.TemporaryDirectory() as home, patch.object(runner.Path, "home", return_value=Path(home)):
            path = Path(home) / ".cache/sophia-render-tests/locks"
            path.mkdir(parents=True)
            victim = Path(home) / "victim"
            victim.write_text("untouched")
            (path / "0000:16:00.0").symlink_to(victim)
            with self.assertRaises(OSError):
                runner.acquire_lock("0000:16:00.0")
            self.assertEqual(victim.read_text(), "untouched")


class Supervision(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.binary = self.root / "program"
        self.binary.write_bytes(b"frozen executable bytes")
        self.binary.chmod(0o700)
        self.args = argparse.Namespace(pci="0000:16:00.0", sha256=runner.digest(self.binary),
                                       output=self.root / "receipt", timeout=0.1,
                                       command=["--", str(self.binary), "--ignored"])
        self.device = {"pci": self.args.pci, "node": "/dev/dri/renderD129", "major": 226, "minor": 129}

    def invoke(self, exit=0, admitted=True, timeout=False):
        receipt = self.args.output
        child = SimpleNamespace(pid=12345)
        child.code = None
        child.waits = 0

        def wait(timeout=None):
            child.waits += 1
            if timed_out and child.waits == 1:
                raise subprocess.TimeoutExpired("bwrap", timeout)
            child.code = exit
            return exit

        timed_out = timeout
        child.wait = wait
        child.poll = lambda: child.code

        def popen(*args, **kwargs):
            self.assertEqual(kwargs["stdin"], subprocess.DEVNULL)
            self.assertTrue(kwargs["close_fds"])
            self.assertTrue(kwargs["start_new_session"])
            self.assertEqual(kwargs["env"], {"PATH": "/usr/bin:/bin"})
            if admitted:
                runner.write_json(receipt / "artifacts/admission.json", {"status": "admitted"})
            return child

        with contextlib.ExitStack() as stack:
            stack.enter_context(patch.object(runner, "select_device", return_value=self.device))
            stack.enter_context(patch.object(runner.shutil, "which", return_value="/bwrap"))
            stack.enter_context(patch.object(runner, "acquire_lock", side_effect=lambda _: os.open(os.devnull, os.O_RDONLY)))
            stack.enter_context(patch.object(runner.subprocess, "Popen", side_effect=popen))
            kill = stack.enter_context(patch.object(runner.os, "killpg"))
            stack.enter_context(contextlib.redirect_stdout(io.StringIO()))
            code = runner.run(self.args)
        return code, json.loads((receipt / "result.json").read_text()), kill

    def test_success_requires_admission_and_freezes_exact_bytes(self):
        code, result, kill = self.invoke()
        self.assertEqual(code, 0)
        self.assertEqual(result["status"], "completed")
        self.assertEqual((self.args.output / "test").read_bytes(), self.binary.read_bytes())
        config = json.loads((self.args.output / "config.json").read_text())
        self.assertEqual(config["arguments"], ["--ignored"])
        kill.assert_not_called()

    def test_zero_exit_without_admission_fails(self):
        code, result, _ = self.invoke(admitted=False)
        self.assertEqual(code, 2)
        self.assertIn("without device admission", result["reason"])

    def test_failure_is_preserved(self):
        code, result, _ = self.invoke(exit=101)
        self.assertEqual(code, 101)
        self.assertEqual(result["status"], "exited")

    def test_timeout_kills_the_owned_group_and_keeps_receipt(self):
        code, result, kill = self.invoke(exit=-9, timeout=True)
        self.assertEqual(code, 124)
        self.assertEqual(result["status"], "timeout")
        kill.assert_called_once_with(12345, signal.SIGKILL)

    def test_wrong_hash_refused_before_launch(self):
        self.args.sha256 = "0" * 64
        with patch.object(runner.subprocess, "Popen") as process:
            with self.assertRaisesRegex(ValueError, "hash mismatch"):
                runner.run(self.args)
            process.assert_not_called()

    def test_existing_receipt_never_overwritten(self):
        self.args.output.mkdir()
        marker = self.args.output / "result.json"
        marker.write_text("old result")
        with self.assertRaises(FileExistsError):
            self.invoke()
        self.assertEqual(marker.read_text(), "old result")

    def test_missing_sandbox_has_no_fallback(self):
        with patch.object(runner, "select_device", return_value=self.device), \
                patch.object(runner.shutil, "which", return_value=None):
            with self.assertRaisesRegex(ValueError, "no unconfined fallback"):
                runner.run(self.args)
        self.assertFalse(self.args.output.exists())


if __name__ == "__main__":
    unittest.main()
