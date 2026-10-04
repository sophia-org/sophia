"""The guest image identity check must pass only when every expected path is
in the image as a regular file with exactly the expected bytes, and record
what the image actually holds. Images here are small newc cpio archives
compressed with zstd, as dracut writes them. They need dracut's lsinitrd,
cpio and zstd, the QEMU fixture's own prerequisites; without them the tests
are skipped by name rather than made mandatory for every gate. The real-image
controls are run and recorded with the QEMU evidence."""
from pathlib import Path
import hashlib
import os
import shutil
import subprocess
import tempfile
import unittest

CHECK = Path(__file__).resolve().parents[1] / "qemu_image_identity.sh"
MISSING = [tool for tool in ("lsinitrd", "cpio", "zstd") if shutil.which(tool) is None]


def digest(data):
    return hashlib.sha256(data).hexdigest()


@unittest.skipIf(MISSING, "QEMU image prerequisites unavailable: " + ", ".join(MISSING))
class ImageIdentityTest(unittest.TestCase):
    def setUp(self):
        self.work = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.work)

    def image(self, files, links=()):
        root = self.work / "root"
        shutil.rmtree(root, ignore_errors=True)
        for path, data in files.items():
            (root / path).parent.mkdir(parents=True, exist_ok=True)
            (root / path).write_bytes(data)
        for path, target in links:
            (root / path).parent.mkdir(parents=True, exist_ok=True)
            os.symlink(target, root / path)
        image = self.work / "image.img"
        listing = subprocess.run(["find", "."], cwd=root, capture_output=True, check=True).stdout
        archive = subprocess.run(["cpio", "-o", "-H", "newc", "--quiet"], cwd=root, input=listing,
                                 capture_output=True, check=True).stdout
        image.write_bytes(subprocess.run(["zstd", "-q", "-c"], input=archive,
                                         capture_output=True, check=True).stdout)
        return image

    def check(self, image, expected):
        path = self.work / "expected.txt"
        path.write_text(expected)
        out = self.work / "actual.txt"
        result = subprocess.run(["bash", str(CHECK), str(image), str(path), str(out)],
                                capture_output=True, text=True, timeout=120, check=False)
        return result, out.read_text() if out.exists() else ""

    def test_exact_bytes_pass_and_are_recorded(self):
        image = self.image({"usr/bin/a": b"alpha", "usr/bin/b": b"beta", "usr/bin/other": b"x"})
        expected = f"{digest(b'alpha')}  usr/bin/a\n{digest(b'beta')}  usr/bin/b\n"
        result, actual = self.check(image, expected)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(actual, expected)

    def test_changed_bytes_fail_and_record_what_the_image_holds(self):
        image = self.image({"usr/bin/a": b"alpha-stripped", "usr/bin/b": b"beta"})
        result, actual = self.check(image, f"{digest(b'alpha')}  usr/bin/a\n{digest(b'beta')}  usr/bin/b\n")
        self.assertEqual(result.returncode, 1)
        self.assertIn("image usr/bin/a is " + digest(b"alpha-stripped"), result.stderr)
        self.assertIn(f"{digest(b'alpha-stripped')}  usr/bin/a", actual)

    def test_missing_or_linked_paths_fail(self):
        # b links to a, which is unpacked too: only the link itself may fail.
        image = self.image({"usr/bin/a": b"alpha"}, links=[("usr/bin/b", "a")])
        result, actual = self.check(
            image, f"{digest(b'alpha')}  usr/bin/a\n{digest(b'alpha')}  usr/bin/b\n{digest(b'c')}  usr/bin/c\n")
        self.assertEqual(result.returncode, 1)
        self.assertIn("image lacks regular file usr/bin/b", result.stderr)
        self.assertIn("image lacks regular file usr/bin/c", result.stderr)
        self.assertEqual(actual, f"{digest(b'alpha')}  usr/bin/a\nmissing  usr/bin/b\nmissing  usr/bin/c\n")

    def test_malformed_or_unsafe_expectations_are_refused(self):
        image = self.image({"usr/bin/a": b"alpha"})
        for expected in ("", "abc  usr/bin/a\n", f"{digest(b'alpha')}  /usr/bin/a\n",
                         f"{digest(b'alpha')}  usr/../etc/a\n", f"{digest(b'alpha')} usr/bin/a\n"):
            with self.subTest(expected=expected):
                result, _ = self.check(image, expected)
                self.assertEqual(result.returncode, 1)


if __name__ == "__main__":
    unittest.main()
