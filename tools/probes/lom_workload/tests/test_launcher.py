"""Actual launcher control flow with fake build/proof/session executables.

No real source checkout, binary, device, endpoint, credential or VT is used.
The real transcript verifiers run; signatures/builds and native results do not.
"""

import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from fixture import transcript, encode


class LauncherTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.base = Path(self.temp.name)
        repo = Path(__file__).resolve().parents[4]
        self.root = self.base / "source"
        self.tools = self.root / "tools"
        self.tools.mkdir(parents=True)
        shutil.copyfile(repo / "tools/run_current_lom_panel_gate_tty4.sh", self.tools / "run.sh")
        shutil.copyfile(repo / "tools/verify_lom_panel_native_gate.sh", self.tools / "verify_lom_panel_native_gate.sh")
        (self.tools / "verify_lom_panel_native_gate.sh").chmod(0o700)
        shutil.copytree(repo / "tools/probes/lom_workload", self.tools / "probes/lom_workload")
        (self.tools / "fixtures").mkdir()
        for name in ("lom_panel_core.kdl", "lom_panel_desktop.kdl", "lom_workload_budgets.json"):
            shutil.copyfile(repo / "tools/fixtures" / name, self.tools / "fixtures" / name)
        self.fakebin = self.base / "bin"
        self.fakebin.mkdir()
        self.lom = self.base / "lom"
        config = self.lom / "examples/minimal/live-shell.kdl"
        config.parent.mkdir(parents=True)
        config.write_text("fixture-only config\n")
        for path in (self.root / "target/release/sophia", self.base / "target/release/lom", self.base / "hagia"):
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("not an executable; fixture hash input only\n")
        self.script(self.fakebin / "tty", 'printf "%s\\n" "${TEST_TTY:-/dev/tty4}"')
        self.script(self.fakebin / "git", '''
case "$*" in
    *"status --short"*) printf "%s" "${TEST_DIRTY:-}" ;;
    *"rev-parse HEAD"*) printf '%064d\\n' 1 ;;
    *"verify-commit HEAD"*) exit 0 ;;
    *) exit 99 ;;
esac''')
        self.script(self.fakebin / "cargo", 'echo build >> "$TEST_TRACE"')
        self.wm_profile = self.base / "wm.kdl"
        self.wm_profile.write_text('schema 1\nshortcut { profile "operator"; bind "Super+4" "policy:focus-workspace" "7"; }\n')
        # Configuration executables are supplied effects in this launcher test.
        # Rust desktop_probe controls cover the real parser/composition policy.
        self.script(self.root / "target/release/sophia", '''
[[ "$1" == config ]]
case "$2" in
  print-effective) [[ "$3" == --desktop-profile="$SOPHIA_DESKTOP_PROFILE" ]]; cat "$SOPHIA_DESKTOP_PROFILE" ;;
  check) [[ -f "${3#--desktop-profile=}" ]] ;;
  *) exit 99 ;;
esac''')
        examples = self.root / "target/release/examples"
        examples.mkdir()
        self.script(examples / "desktop_profile_probe", '''
[[ "$#" == 2 ]]
cat "$1"
tail -n +2 "$2"''')
        self.script(self.tools / "lom_gpu_content_hardware_proof.sh", '''
echo proof >> "$TEST_TRACE"
[[ "$SOPHIA_LOM_GPU_PROOF_ARM" == 1 ]]
exit "${TEST_PROOF_STATUS:-0}"''')
        self.script(self.tools / "run_sophia_session.sh", '''
echo session >> "$TEST_TRACE"
[[ "$#" == 2 && "$1" == --max-runtime-ms=90000 ]]
[[ "$2" == --shell-process="$SOPHIA_LOM_TARGET_DIR/release/lom" ]]
[[ "$SOPHIA_SESSION_WATCHDOG_SECONDS" == 110 && "$SOPHIA_SESSION_STARTUP" == none ]]
[[ "$SOPHIA_REQUIRE_LOCAL_VT" == true && "$SOPHIA_MANAGE_KEYD" == true ]]
mkdir -p "$SOPHIA_DIAGNOSTIC_DIR"
cp "$TEST_HOST" "$SOPHIA_DIAGNOSTIC_DIR/events.0.log"
cp "$TEST_CLIENT" "$SOPHIA_UNTRUSTED_SESSION_OUTPUT_LOG"
if [[ "${TEST_RECOVERY:-yes}" == yes ]]; then
    printf '%s\\n' 'sophia_tty_recovery schema=3 termios_restored=true done=true' \
      'sophia_tty_recovery_verification schema=1 keyd_restored=true' > "$SOPHIA_DIAGNOSTIC_DIR/recovery.log"
fi
if [[ "${TEST_CHANGE_INPUT:-no}" == yes ]]; then echo changed >> "$SOPHIA_SHELL_CONFIG"; fi
exit "${TEST_SESSION_STATUS:-0}"''')
        host, client = transcript()
        native = "\n".join([
            "sophia_live_shell_gpu schema=1 status=granted device=fixture",
            "sophia_live_wm_configuration schema=2 status=committed generation=1",
            "sophia_live_shell_content schema=1 status=outputs outputs=2",
            *[f"sophia_live_shell_content schema=1 status=presented output={o} candidate_generation={g} done=true"
              for o in (1, 2) for g in (1, 2)],
        ]) + "\n"
        (self.base / "host.log").write_text(native + encode(host))
        (self.base / "client.log").write_text(encode(client))
        self.evidence = self.base / "evidence"
        self.env = {**os.environ, "PATH": str(self.fakebin) + ":/usr/bin:/bin",
                    "SOPHIA_LOM_SOURCE": str(self.lom), "SOPHIA_LOM_TARGET_DIR": str(self.base / "target"),
                    "SOPHIA_HAGIA_BIN": str(self.base / "hagia"), "SOPHIA_LOM_NATIVE_GATE_ARM": "1",
                    "SOPHIA_DESKTOP_PROFILE": str(self.wm_profile),
                    "SOPHIA_LOM_NATIVE_EVIDENCE_DIR": str(self.evidence),
                    "TEST_TRACE": str(self.base / "trace"), "TEST_HOST": str(self.base / "host.log"),
                    "TEST_CLIENT": str(self.base / "client.log")}
        for name in ("SOPHIA_LOM_CONFIG", "SOPHIA_LOM_CORE_CONFIG", "DISPLAY", "WAYLAND_DISPLAY"):
            self.env.pop(name, None)

    def script(self, path, body):
        path.write_text("#!/usr/bin/env bash\nset -euo pipefail\n" + body + "\n")
        path.chmod(0o700)

    def run_launcher(self, **env):
        return subprocess.run(["bash", str(self.tools / "run.sh")], env={**self.env, **env},
                              capture_output=True, text=True, timeout=10, umask=0o002)

    def test_generated_profiles_are_private_under_group_writable_umask(self):
        result = self.run_launcher()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        for name in ("wm-profile.kdl", "desktop.kdl"):
            self.assertEqual((self.evidence / name).stat().st_mode & 0o777, 0o600)

    def test_normal_exit_runs_both_real_verifiers(self):
        result = self.run_launcher()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        report = json.loads((self.evidence / "workload-verification.json").read_text())
        self.assertEqual(report["status"], "pass")
        self.assertEqual(report["memory"]["slot_bound"], 4)
        self.assertEqual((self.evidence / "native-outcome.txt").read_text(), "native_exit_status=0\n")
        self.assertEqual((self.base / "trace").read_text().splitlines(), ["build", "build", "build", "proof", "session"])
        self.assertEqual((self.evidence / "wm-profile.kdl").read_bytes(), self.wm_profile.read_bytes())
        self.assertIn('bind "Super+4" "policy:focus-workspace" "7"', (self.evidence / "desktop.kdl").read_text())
        self.assertIn("wm_profile_sha256=", (self.evidence / "identity.manifest").read_text())
        self.assertIn("probe_overrides_sha256=", (self.evidence / "identity.manifest").read_text())

    def test_missing_selected_wm_profile_never_launches_proof_or_session(self):
        result = self.run_launcher(SOPHIA_DESKTOP_PROFILE=str(self.base / "missing.kdl"))
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("existing absolute WM profile", result.stderr)
        self.assertNotIn("proof", (self.base / "trace").read_text())
        self.assertNotIn("session", (self.base / "trace").read_text())

    def test_preconditions_stop_before_build_and_proof(self):
        for env in ({"TEST_TTY": "/dev/pts/1"}, {"SOPHIA_LOM_NATIVE_GATE_ARM": "0"}, {"TEST_DIRTY": " M fixture"}):
            with self.subTest(env=env):
                self.assertNotEqual(self.run_launcher(**env).returncode, 0)
                self.assertFalse((self.base / "trace").exists())

    def test_failed_proof_never_launches_session(self):
        self.assertNotEqual(self.run_launcher(TEST_PROOF_STATUS="1").returncode, 0)
        self.assertIn("proof", (self.base / "trace").read_text())
        self.assertNotIn("session", (self.base / "trace").read_text())

    def test_watchdog_is_failure_and_preserves_existing_evidence(self):
        self.assertNotEqual(self.run_launcher(TEST_SESSION_STATUS="124").returncode, 0)
        self.assertEqual((self.evidence / "native-outcome.txt").read_text(), "native_exit_status=124\n")
        old = (self.base / "trace").read_bytes()
        self.assertNotEqual(self.run_launcher().returncode, 0)
        self.assertEqual((self.base / "trace").read_bytes(), old)

    def test_missing_recovery_fails(self):
        result = self.run_launcher(TEST_RECOVERY="no")
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse((self.evidence / "workload-verification.json").exists())

    def test_changed_inputs_fail(self):
        result = self.run_launcher(TEST_CHANGE_INPUT="yes")
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse((self.evidence / "workload-verification.json").exists())

    def test_missing_workload_action_fails_real_verifier(self):
        path = self.base / "host.log"
        lines = path.read_text().splitlines()
        lines = [line for line in lines if not ("sophia_shell_action_cause" in line and "event_id=1 " in line)]
        path.write_text("\n".join(lines) + "\n")
        self.assertNotEqual(self.run_launcher().returncode, 0)
        result = json.loads((self.evidence / "workload-verification.json").read_text())
        self.assertEqual(result["status"], "fail")
