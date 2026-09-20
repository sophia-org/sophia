"""The selection reads purposes from the suite; it never invents them."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

HERE = Path(__file__).resolve().parent


def fabricate(root, case, directory, assertions):
    source = root / 'xts5' / directory / case / f'{case}.m'
    source.parent.mkdir(parents=True)
    body = f'>># {case}\n' + ''.join(f'>>ASSERTION Good A\n>>CODE\nx{n}();\n' for n in range(assertions))
    source.write_text(body)


class SelectionTests(unittest.TestCase):
    def run_select(self, root, *cases, install=False):
        manifest = root / 'manifest.json'
        command = [sys.executable, '-B', str(HERE / 'xts_select.py'), '--xts-root', str(root),
                   '--scenario', 'selected-core', '--manifest', str(manifest)]
        for case in cases:
            command += ['--case', case]
        if install:
            command.append('--install')
        result = subprocess.run(command, capture_output=True, text=True)
        return result, manifest

    def test_purposes_are_counted_from_the_sources_and_paths_are_tets(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            fabricate(root, 'XDestroyWindow', 'Xlib3', 3)
            fabricate(root, 'XInternAtom', 'Xlib4', 2)
            result, manifest = self.run_select(root, 'XDestroyWindow', 'XInternAtom', install=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            rows = json.loads(manifest.read_text())
            self.assertEqual(rows, [
                {'case': '/tset/Xlib3/XDestroyWindow/Test', 'purpose': 1},
                {'case': '/tset/Xlib3/XDestroyWindow/Test', 'purpose': 2},
                {'case': '/tset/Xlib3/XDestroyWindow/Test', 'purpose': 3},
                {'case': '/tset/Xlib4/XInternAtom/Test', 'purpose': 1},
                {'case': '/tset/Xlib4/XInternAtom/Test', 'purpose': 2},
            ])
            scenario = (root / 'xts5/tet_scen.selected-core').read_text()
            self.assertEqual(scenario.splitlines()[0], 'selected-core')
            self.assertIn('\t/tset/Xlib3/XDestroyWindow/Test', scenario)
            self.assertIn('\t/tset/Xlib4/XInternAtom/Test', scenario)

    def test_a_case_without_sources_or_assertions_is_refused(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            fabricate(root, 'XMapWindow', 'Xlib3', 0)
            missing, _ = self.run_select(root, 'XUnmapWindow')
            self.assertEqual(missing.returncode, 1)
            self.assertIn('no .m source', missing.stderr)
            empty, manifest = self.run_select(root, 'XMapWindow')
            self.assertEqual(empty.returncode, 1)
            self.assertIn('no >>ASSERTION', empty.stderr)
            self.assertFalse(manifest.exists())

    def test_duplicate_and_malformed_cases_are_refused(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            fabricate(root, 'XMapWindow', 'Xlib3', 1)
            duplicate, _ = self.run_select(root, 'XMapWindow', 'XMapWindow')
            self.assertEqual(duplicate.returncode, 1)
            malformed, _ = self.run_select(root, '../etc')
            self.assertEqual(malformed.returncode, 1)


if __name__ == '__main__':
    unittest.main()
