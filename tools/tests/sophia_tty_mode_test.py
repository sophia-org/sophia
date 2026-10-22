"""The VT mode helper reuses only an inherited descriptor for its exact target."""
import os
from pathlib import Path
import subprocess
import sys
import unittest

TOOLS = Path(__file__).resolve().parents[1]
PROBE = (
    "import os, sys; sys.path.insert(0, sys.argv[1]); import sophia_tty_mode as m\n"
    "try:\n"
    "    tty = m.open_tty(sys.argv[2])\n"
    "except PermissionError:\n"
    "    print('denied'); raise SystemExit(0)\n"
    "same = os.path.samestat(os.fstat(tty.fileno()), os.fstat(0))\n"
    "print('inherited' if same else 'other')\n"
)


def denied_pty():
    """A pseudo-terminal whose device node no longer admits an open by path."""
    controller, terminal = os.openpty()
    path = os.ttyname(terminal)
    os.chmod(path, 0)
    return controller, terminal, path


def probe(stdin: int, target: str) -> str:
    result = subprocess.run([sys.executable, "-B", "-c", PROBE, str(TOOLS), target],
                            stdin=stdin, capture_output=True, text=True, timeout=30, check=True)
    return result.stdout.strip()


@unittest.skipIf(os.geteuid() == 0, "root opens a mode-000 node")
class OpenTty(unittest.TestCase):
    def test_matching_inherited_terminal_survives_a_denied_path_open(self):
        controller, terminal, path = denied_pty()
        try:
            with self.assertRaises(PermissionError):
                open(path, "rb+", buffering=0)
            self.assertEqual(probe(terminal, path), "inherited")
        finally:
            os.close(controller)
            os.close(terminal)

    def test_a_different_target_is_opened_by_path_and_still_refused(self):
        controller, terminal, _ = denied_pty()
        other_controller, other_terminal, other_path = denied_pty()
        try:
            self.assertEqual(probe(terminal, other_path), "denied")
        finally:
            for fd in (controller, terminal, other_controller, other_terminal):
                os.close(fd)


if __name__ == "__main__":
    unittest.main()
