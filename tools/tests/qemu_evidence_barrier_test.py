"""The host barrier before the provider scenario's final Return must accept
only a button_routed record written after its pre-click line count: never
an earlier record, a suppressed or observed button, or no record at all."""
from pathlib import Path
import subprocess
import tempfile
import unittest

BARRIER = Path(__file__).resolve().parents[1] / "qemu_evidence_barrier.sh"
PATTERN = "^sophia_live_session_pointer schema=2 status=button_routed count=[1-9][0-9]*$"
ROUTED = "sophia_live_session_pointer schema=2 status=button_routed count=1"
MARKER = "sophia_qemu_gtk_pointer schema=1 status=sent phase=focused_select source=qmp clicks=1"
TIMEOUT = "sophia_qemu_gtk schema=1 status=failed reason=select_route_timeout scenario=session-lock-provider"


def bash(script, *args):
    return subprocess.run(["bash", "-c", f'. "{BARRIER}"; {script}', "barrier", *args],
                          capture_output=True, text=True, check=False)


class BarrierTest(unittest.TestCase):
    def evidence(self, *lines):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        path = Path(directory.name) / "evidence.log"
        path.write_text("".join(f"{line}\n" for line in lines))
        return path

    def after(self, path, anchor):
        return bash('evidence_after_line "$1" "$2" "$3"', str(path), str(anchor), PATTERN).returncode

    def test_only_a_record_after_the_pre_click_count_satisfies_it(self):
        self.assertEqual(self.after(self.evidence("a", ROUTED, "b"), 3), 1)
        self.assertEqual(self.after(self.evidence("a", "b", "c", ROUTED, MARKER), 3), 0)
        self.assertEqual(self.after(self.evidence("a", "b", "c", MARKER, ROUTED), 3), 0)
        self.assertEqual(self.after(self.evidence("a", ROUTED, "b", MARKER), 3), 1)

    def test_suppressed_or_observed_buttons_do_not_satisfy_it(self):
        path = self.evidence(
            "a", "b", "c",
            "sophia_live_session_pointer schema=8 status=button_suppressed reason=policy mode=full count=2",
            "sophia_live_session_pointer schema=2 status=button_observed count=2",
            "sophia_live_session_pointer schema=2 status=button_routed count=0")
        self.assertEqual(self.after(path, 3), 1)

    def test_the_wait_fails_by_name_without_a_record_and_passes_with_one(self):
        wait = ('EVIDENCE_FILE="$1" QEMU_PID=2147483646 SCENARIO=session-lock-provider; '
                'wait_for_after_line 3 "$2" select_route_timeout')
        path = self.evidence("a", ROUTED, "b", MARKER)
        result = bash(wait, str(path), PATTERN)
        self.assertEqual(result.returncode, 1)
        self.assertEqual(path.read_text().splitlines()[-1], TIMEOUT)
        self.assertEqual(bash(wait, str(self.evidence("a", "b", "c", ROUTED)), PATTERN).returncode, 0)


if __name__ == "__main__":
    unittest.main()
