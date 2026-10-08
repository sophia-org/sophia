"""Actual Python/bubblewrap startup with a private root, never host root/PAM."""
import hashlib
import json
import os
from pathlib import Path
import py_compile
import shutil
import subprocess
import sys
import tempfile
import unittest


SOURCE = Path(__file__).resolve().parents[1] / "development_seat"
sys.path.insert(0, str(SOURCE))
import bootstrap


class Bootstrap(unittest.TestCase):
    def test_unchecked_bytecode_is_not_used_under_the_launch_flags(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            module = root / "fixture_module.py"
            module.write_text("value = 'unreviewed cached code'\n")
            py_compile.compile(str(module), doraise=True,
                               invalidation_mode=py_compile.PycInvalidationMode.UNCHECKED_HASH)
            module.write_text("value = 'pinned source'\n")
            code = "import sys;sys.path.insert(0,sys.argv[1]);import fixture_module;print(fixture_module.value)"
            def run(flags):
                return subprocess.run([sys.executable, *flags, "-c", code, str(root)],
                                      stdin=subprocess.DEVNULL, capture_output=True, text=True,
                                      check=True, timeout=5).stdout.strip()
            self.assertEqual(run(["-I", "-B"]), "unreviewed cached code")
            self.assertEqual(run(bootstrap.PYTHON_FLAGS), "pinned source",
                             "startup must ignore unchecked source-adjacent bytecode")

    def test_real_bootstrap_verifies_before_importing_any_bundle_code(self):
        # bwrap maps only this unprivileged UID to namespace root. /etc and /
        # are fresh private tmpfs mounts; no installed authority is invoked.
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            bundle = root / "bundle"
            bundle.mkdir()
            shutil.copyfile(SOURCE / "bootstrap.py", bundle / "bootstrap.py")
            (bundle / "owner.py").write_text(
                "import sys; sys.path.insert(0, '/bundle'); import payload; print(payload.value)\n")
            payload = bundle / "payload.py"
            payload.write_text("value = 'VERIFIED SOURCE'\n")
            config = {"bundle": "/bundle", "tools": {}, "files": [
                {"path": "/bundle/" + p.name, "sha256": hashlib.sha256(p.read_bytes()).hexdigest()}
                for p in bundle.iterdir()]}
            settings = root / "launch.json"
            settings.write_text(json.dumps(config))
            argv = ["bwrap", "--unshare-all", "--uid", "0", "--gid", "0", "--die-with-parent",
                    "--tmpfs", "/", "--ro-bind", "/usr", "/usr", "--symlink", "usr/lib", "/lib",
                    "--symlink", "usr/lib", "/lib64", "--symlink", "usr/bin", "/bin",
                    "--dev", "/dev", "--proc", "/proc", "--dir", "/etc",
                    "--dir", "/etc/sophia-development", "--ro-bind", str(settings),
                    "/etc/sophia-development/launch.json", "--ro-bind", str(bundle), "/bundle",
                    "--clearenv", "--setenv", "LANG", "C.UTF-8", "--chdir", "/"]
            if Path("/nix/store").is_dir():
                argv += ["--ro-bind", "/nix/store", "/nix/store"]
            argv += ["--", sys.executable, *bootstrap.PYTHON_FLAGS, "/bundle/bootstrap.py", "owner"]
            def run(reason=None):
                result = subprocess.run(argv, stdin=subprocess.DEVNULL, capture_output=True,
                                        text=True, timeout=10)
                if reason is None:
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertEqual(result.stdout.strip(), "VERIFIED SOURCE")
                else:
                    self.assertNotEqual(result.returncode, 0)
                    self.assertEqual(result.stdout, "", "refused bundle code must not execute")
                    self.assertIn(reason, result.stderr)
            run()
            payload.write_text("print('UNVERIFIED IMPORT'); value = 'changed'\n")
            run("bundle file hash")
            payload.write_text("value = 'VERIFIED SOURCE'\n")
            payload.chmod(0o666)
            run("untrusted bootstrap path")
            payload.chmod(0o644)
            extra = bundle / "extra.py"
            extra.write_text("print('UNVERIFIED IMPORT')\n")
            run("bundle directory inventory")
            extra.unlink()
            py_compile.compile(str(payload), doraise=True,
                               invalidation_mode=py_compile.PycInvalidationMode.UNCHECKED_HASH)
            run("bundle directory inventory")
            shutil.rmtree(bundle / "__pycache__")
            payload.unlink()
            run("bundle directory inventory")

    def test_bytecode_and_symlinks_are_refused_even_if_pinned(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            path = root / "cache.pyc"
            path.write_bytes(b"inert")
            config = {"bundle": str(root), "tools": {}, "files": [
                {"path": str(path), "sha256": hashlib.sha256(b"inert").hexdigest()}]}
            with self.assertRaisesRegex(ValueError, "bytecode cache"):
                bootstrap.inventory(config)
            path.unlink()
            path.symlink_to("/dev/null")
            with self.assertRaisesRegex(ValueError, "link or bytecode"):
                bootstrap.inventory(config)


if __name__ == "__main__":
    unittest.main()
