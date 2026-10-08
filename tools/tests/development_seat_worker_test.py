"""Exercise worker cleanup with real child processes and a fake login authority.

No host PAM stack, login1 call, credential transition or device is used. The
separate private-PAM and namespace controls exercise those CPU seams; this
test covers their ordering in the worker, including failure before admission.
"""
from contextlib import ExitStack
import copy
import json
import os
import signal
import ctypes
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import Mock, patch

from development_seat_login_test import fixture_config


SOURCE = Path(__file__).resolve().parents[1] / "development_seat"
sys.path.insert(0, str(SOURCE))
import custody
import owner
import worker
import pam


class WorkerWiring(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temporary = tempfile.TemporaryDirectory(prefix="development-worker-")
        cls.addClassCleanup(cls.temporary.cleanup)
        cls.directory = Path(cls.temporary.name)
        cls.custody_executable = cls.directory / "custody"
        subprocess.run(["cc", "-std=c11", "-Wall", "-Wextra", "-Werror",
                        str(SOURCE / "custody_exec.c"), "-o", str(cls.custody_executable)],
                       check=True, capture_output=True, timeout=30)

    def scenario(self, scenario):
        with tempfile.TemporaryDirectory(dir=self.directory) as temporary, ExitStack() as stack:
            base = Path(temporary)
            run = base / "run"
            run.mkdir()
            config = fixture_config()
            config.update(uid=os.getuid(), gid=os.getgid(), output_root=str(base))
            config["tools"]["custody"]["path"] = str(self.custody_executable)
            config["tools"]["guard"]["path"] = "/usr/bin/env"
            devices = {"nodes": [], "pci": "fixture"}
            observed = {"session": "fixture-login", "seat": config["seat"]}
            deadline = time.monotonic() + (0.25 if scenario == "timeout" else 5)
            worker.write(run / "declaration.json", {"config": config, "inventory": devices,
                                                   "bus": {}, "deadline": deadline})
            events, children = [], []
            real_child = custody.Child

            class Child(real_child):
                def __init__(self, *args, **kwargs):
                    super().__init__(*args, **kwargs)
                    children.append(self)
                    events.append("child")

                def finish(self, *args, **kwargs):
                    if scenario == "unreaped":
                        raise TimeoutError("fixture unreapable sandbox")
                    if scenario == "term-during-cleanup":
                        os.kill(os.getpid(), signal.SIGTERM)
                    result = super().finish(*args, **kwargs)
                    events.append("reaped")
                    return result

            class Pam:
                def __init__(self, *args):
                    events.append("pam-created")

                def open(self):
                    events.append("pam-open")
                    if scenario == "pam-failure":
                        raise ValueError("fixture partial PAM failure")

                def getenv(self, name):
                    return {"XDG_SESSION_ID": "fixture-login", "XDG_SEAT": config["seat"]}[name]

                def close(self):
                    # Cleanup of a Session must precede release of its login.
                    if any(child.process.returncode is None for child in children):
                        raise AssertionError("PAM closed before child reap")
                    events.append("pam-close")
                    if scenario == "term-during-cleanup":
                        os.kill(os.getpid(), signal.SIGTERM)

            def command(config, run, login, inventory, fd, **kwargs):
                record = {"schema": 1, "status": "admitted", "session": login["session"],
                          "inventory": inventory}
                if scenario in ("identity-mismatch", "unreaped", "term-during-cleanup"):
                    record["session"] = "daily-login"
                code = f"import os,time; os.write({fd},{(json.dumps(record) + chr(10)).encode()!r}); os.close({fd}); "
                if scenario == "no-admission":
                    code = "raise SystemExit(5)"
                elif scenario in ("timeout", "identity-mismatch", "unreaped", "term-during-cleanup"):
                    code += "time.sleep(30)"
                else:
                    code += f"raise SystemExit({3 if scenario == 'nonzero' else 0})"
                return [sys.executable, "-I", "-B", "-c", code]

            stack.enter_context(patch.object(worker.configuration, "load", return_value=config))
            stack.enter_context(patch.object(worker.configuration, "trusted"))
            stack.enter_context(patch.object(worker.login, "Login", return_value=Mock(session=lambda _: None)))
            stack.enter_context(patch.object(worker.login, "attest", return_value=observed))
            stack.enter_context(patch.object(worker.inventory, "discover", return_value=devices))
            stack.enter_context(patch.object(worker, "Pam", Pam))
            stack.enter_context(patch.object(worker.sandbox, "command", side_effect=command))
            stack.enter_context(patch.object(worker.custody, "Child", Child))
            stack.enter_context(patch.object(worker.os, "chown"))
            stack.enter_context(patch.object(sys, "argv", ["worker.py", str(run)]))
            previous_mask = signal.pthread_sigmask(signal.SIG_BLOCK, set())
            previous_handler = signal.signal(signal.SIGTERM,
                lambda *_: (_ for _ in ()).throw(InterruptedError("fixture TERM")))
            try:
                result = worker.main()
            finally:
                # Production stays masked until process exit. This in-process
                # control consumes its pending TERM before restoring the test.
                signal.pthread_sigmask(signal.SIG_BLOCK, {signal.SIGTERM})
                while signal.sigtimedwait({signal.SIGTERM}, 0) is not None:
                    pass
                signal.signal(signal.SIGTERM, previous_handler)
                signal.pthread_sigmask(signal.SIG_SETMASK, previous_mask)
                for child in children:
                    if child.pidfd is not None:
                        real_child.finish(child, terminate=True)
            report = json.loads((run / "worker-result.json").read_text())
            if scenario == "unreaped":
                self.assertEqual(result, 2)
                self.assertEqual(report.get("pam_cleanup"), "skipped_unreaped_child",
                                 "unreaped child must skip PAM teardown")
                self.assertIn("unreapable", report["cleanup_error"])
                self.assertNotIn("pam-close", events, "unreaped child must not trigger PAM close")
                return
            self.assertEqual(events[-1], "pam-close")
            self.assertTrue(all(child.pidfd is None and child.process.returncode is not None for child in children))
            self.assertNotIn("cleanup_error", report)
            self.assertNotIn("pam_cleanup_error", report)
            if scenario == "success":
                self.assertEqual(result, 0, report)
                self.assertEqual(report["status"], "completed")
                self.assertEqual(events, ["pam-created", "pam-open", "child", "reaped", "pam-close"])
            else:
                self.assertEqual(result, 2, report)
                self.assertEqual(report["status"], "failed")
                self.assertTrue(report["reason"])
            if scenario in ("pam-failure", "identity-mismatch", "no-admission"):
                self.assertFalse((run / "admission.json").exists())

    def test_success_and_every_refusal_reap_before_closing_pam(self):
        for scenario in ("success", "nonzero", "identity-mismatch", "no-admission", "timeout", "pam-failure"):
            with self.subTest(scenario=scenario):
                self.scenario(scenario)

    def test_unreaped_sandbox_never_closes_pam(self):
        self.scenario("unreaped")

    def test_repeated_term_does_not_interrupt_cleanup_or_its_receipt(self):
        self.scenario("term-during-cleanup")


class OwnedLoginCleanup(unittest.TestCase):
    def test_term_during_owner_cleanup_cannot_skip_reap_or_result(self):
        with tempfile.TemporaryDirectory() as temporary, ExitStack() as stack:
            config = fixture_config()
            config.update(output_root=temporary, outer_seconds=15.05)
            config["tools"]["custody"]["path"] = "/usr/bin/env"
            children = []
            real_child = custody.Child

            def child(*args, **kwargs):
                value = real_child([sys.executable, "-I", "-c", "import time;time.sleep(30)"], **kwargs)
                children.append(value)
                return value

            def terminate(*args):
                os.kill(os.getpid(), signal.SIGTERM)
                return {"status": "fixture"}

            actual_fstat = os.fstat
            def root_lock(fd):
                values = list(actual_fstat(fd))
                values[4] = 0  # emulate only the root-owned flock file
                return os.stat_result(values)

            stack.enter_context(patch.object(owner.configuration, "trusted"))
            stack.enter_context(patch.object(owner, "preflight", return_value={}))
            stack.enter_context(patch.object(owner.os, "fstat", side_effect=root_lock))
            stack.enter_context(patch.object(owner.custody, "Child", side_effect=child))
            stack.enter_context(patch.object(owner, "observed_owned_login", return_value=None))
            stack.enter_context(patch.object(owner, "terminate_owned_login", side_effect=terminate))
            previous_mask = signal.pthread_sigmask(signal.SIG_BLOCK, set())
            previous_handler = signal.signal(signal.SIGTERM,
                lambda *_: (_ for _ in ()).throw(InterruptedError("owner cleanup TERM")))
            try:
                self.assertEqual(owner.run_once(config, None, {}), 2)
            finally:
                signal.pthread_sigmask(signal.SIG_BLOCK, {signal.SIGTERM})
                while signal.sigtimedwait({signal.SIGTERM}, 0) is not None:
                    pass
                signal.signal(signal.SIGTERM, previous_handler)
                signal.pthread_sigmask(signal.SIG_SETMASK, previous_mask)
                for value in children:
                    real_child.finish(value, terminate=True)
            report, = Path(temporary).glob("*/result.json")
            result = json.loads(report.read_text())
            self.assertNotIn("login_cleanup_error", result, "owner cleanup must defer TERM")
            self.assertEqual(result["login_cleanup"], {"status": "fixture"})
            self.assertNotIn("cleanup_error", result)
            self.assertTrue(all(c.process.returncode is not None for c in children))

    def test_only_the_same_still_owned_login_can_be_terminated(self):
        config = fixture_config()
        child = Mock()
        child.process.pid = 1234
        observed = {"session": "fixture-login", "seat": config["seat"], "uid": config["uid"],
                    "process": {"pid": 1234, "start_ticks": 99, "cgroup": "0::/fixture\n"}}
        for change in (None, "session", "uid", "process", "absent"):
            current = copy.deepcopy(observed)
            if change == "absent":
                current = None
            elif change == "process":
                current["process"]["start_ticks"] += 1
            elif change:
                current[change] = "different"
            with self.subTest(change=change), \
                 patch.object(owner, "observed_owned_login", return_value=current), \
                 patch.object(owner.subprocess, "run", return_value=Mock(returncode=0)) as run:
                result = owner.terminate_owned_login(None, config, child, observed)
                if change is None:
                    self.assertEqual(result["status"], "requested")
                    self.assertEqual(run.call_args.args[0], ["/usr/bin/loginctl", "terminate-session", "fixture-login"])
                else:
                    self.assertEqual(result["status"], "not_attested")
                    run.assert_not_called()


class PamCleanup(unittest.TestCase):
    def test_term_between_close_and_end_is_delayed_until_both_finish(self):
        events = []
        value = pam.Pam.__new__(pam.Pam)
        value.handle, value.opened = ctypes.c_void_p(1), True
        def close(*args):
            events.append("close")
            os.kill(os.getpid(), signal.SIGTERM)
            return 0
        def end(*args):
            events.append("end")
            return 0
        value.lib = Mock(pam_close_session=close, pam_end=end)
        previous = signal.signal(signal.SIGTERM,
            lambda *_: (_ for _ in ()).throw(InterruptedError("PAM TERM")))
        try:
            with self.assertRaisesRegex(InterruptedError, "PAM TERM"):
                value.close()
        finally:
            signal.signal(signal.SIGTERM, previous)
        self.assertEqual(events, ["close", "end"], "TERM must not split PAM teardown")
        self.assertFalse(value.handle.value)


if __name__ == "__main__":
    unittest.main()
