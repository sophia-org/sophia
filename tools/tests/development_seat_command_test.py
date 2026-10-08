"""Admission and command-shape controls; no launch or host metadata lookup."""
import importlib.util
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

from development_seat_login_test import fixture_config


SOURCE = Path(__file__).resolve().parents[1] / "development_seat"
sys.path.insert(0, str(SOURCE))
import deployment
import inventory
import policy
import sandbox


class Commands(unittest.TestCase):
    def test_root_setup_executes_only_the_drop_helper_and_trusted_auditor(self):
        config = fixture_config()
        row = {"session": "c29"}
        devices = {"nodes": [{"node": "/dev/dri/card1"}, {"node": "/dev/dri/renderD129"}]}
        argv = sandbox.command(config, Path("/var/lib/sophia-development/runs/fixture"), row,
                               devices, 9, nix_store=True)
        last = argv.index("--")
        self.assertEqual(argv[last + 1:last + 5],
                         [config["tools"]["drop"]["path"], "1200", "1200", "--"])
        self.assertNotIn(config["tools"]["sophia"]["path"], argv)
        self.assertEqual(argv[last + 5:], [config["tools"]["python"]["path"], "-I", "-B", "-S",
                                         "-X", "pycache_prefix=/dev/null",
                                         config["bundle"] + "/bootstrap.py", "audit", "9"])
        for flag in ("--unshare-pid", "--unshare-ipc", "--unshare-uts", "--clearenv", "--new-session"):
            self.assertIn(flag, argv)
        self.assertNotIn("--unshare-user", argv)
        self.assertNotIn("--unshare-net", argv)
        bound = [(argv[i+1], argv[i+2]) for i, item in enumerate(argv) if item == "--dev-bind"]
        self.assertEqual(bound, [("/dev/dri/card1", "/dev/dri/card1"),
                                 ("/dev/dri/renderD129", "/dev/dri/renderD129")])
        ro = [(argv[i+1], argv[i+2]) for i, item in enumerate(argv) if item == "--ro-bind"]
        self.assertIn(("/run/dbus/system_bus_socket", "/run/dbus/system_bus_socket"), ro)
        self.assertNotIn(("/", "/"), ro)

    def test_session_is_inputless_bounded_and_never_inherits_desktop_environment(self):
        config = fixture_config()
        with patch.dict(os.environ, {"DISPLAY": ":0", "XDG_SESSION_ID": "daily",
                                    "LD_PRELOAD": "wrong", "SOPHIA_TEST_RENDER_NODE": "wrong"}):
            env = sandbox.environment(config, {"session": "c29"})
        self.assertEqual(env["XDG_SESSION_ID"], "c29")
        self.assertEqual(env["LIBSEAT_BACKEND"], "logind")
        self.assertFalse({"DISPLAY", "LD_PRELOAD", "SOPHIA_TEST_RENDER_NODE"} & set(env))
        command = sandbox.session_command(config)
        self.assertIn("--no-input", command)
        self.assertIn("--development-seat=seat-sophia-dev", command)
        self.assertIn("--max-runtime-ms=300000", command)
        self.assertIn("--bubblewrap=" + config["tools"]["nested_bubblewrap"]["path"], command)

    def test_service_has_an_independent_hard_bound_and_parent_death_edge(self):
        config = fixture_config()
        text = deployment.service_run(config)
        self.assertIn("exec /usr/bin/env -i", text)
        self.assertIn("/usr/bin/timeout -s KILL 360 /usr/bin/custody \"$$\" --", text)
        self.assertIn(" --run < /dev/null", text)
        self.assertNotIn("sudo", text)
        self.assertEqual(deployment.proposed_files(config)["/etc/sv/sophia-development/down"], "")


class Inventory(unittest.TestCase):
    def test_uninitialized_foreign_card_refuses_before_any_path_inspection(self):
        rows = [{"initialized": False, "seat": "seat0", "sysfs": "/not/read", "node": None}]
        with patch.object(Path, "resolve", side_effect=AssertionError("must not inspect")):
            with self.assertRaisesRegex(ValueError, "uninitialized"):
                inventory.select(rows, "seat-dev", "0000:16:00.0")

    def test_one_card_and_only_its_exact_render_sibling_are_admitted(self):
        with tempfile.TemporaryDirectory() as temporary:
            drm = Path(temporary) / "0000:16:00.0" / "drm"
            card, render = drm / "card1", drm / "renderD129"
            for path in (card, render):
                path.mkdir(parents=True)
                (path / "device").symlink_to(drm.parent)
            rows = [{"initialized": True, "seat": "seat-dev", "sysfs": str(card), "node": "/dev/dri/card1"},
                    {"initialized": True, "seat": "seat0", "sysfs": "/must/not/inspect", "node": None}]
            with patch.object(inventory, "device_record", side_effect=lambda node, _: {"node": str(node)}):
                selected = inventory.select(rows, "seat-dev", "0000:16:00.0")
                self.assertEqual(selected["nodes"], [{"node": "/dev/dri/card1"}, {"node": "/dev/dri/renderD129"}])
                with self.assertRaisesRegex(ValueError, "exactly one"):
                    inventory.select(rows + [rows[0]], "seat-dev", "0000:16:00.0")
                with self.assertRaisesRegex(ValueError, "PCI"):
                    inventory.select(rows, "seat-dev", "0000:03:00.0")
                (render / "device").unlink()
                other = Path(temporary) / "0000:03:00.0"
                other.mkdir()
                (render / "device").symlink_to(other)
                with self.assertRaisesRegex(ValueError, "sibling"):
                    inventory.select(rows, "seat-dev", "0000:16:00.0")


if __name__ == "__main__":
    unittest.main()
