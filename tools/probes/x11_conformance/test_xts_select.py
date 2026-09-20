"""The selection reads purposes from the suite; it never invents them."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

HERE = Path(__file__).resolve().parent


def fabricate(root, case, directory, assertions, with_data=False):
    source = root / 'xts5' / directory / (f'{case}/{case}.m' if with_data else f'{case}.m')
    source.parent.mkdir(parents=True, exist_ok=True)
    body = f'>># {case}\n' + ''.join(f'>>ASSERTION Good A\n>>CODE\nx{n}();\n' for n in range(assertions))
    source.write_text(body)
    scenario_file = root / 'xts5' / 'tet_scen'
    if not scenario_file.exists():
        scenario_file.write_text('all\n\t"everything"\n\t:include:/scenarios/Xlib3_scen\n\nXlib3\n\t"section"\n\t:include:/scenarios/Xlib3_scen\n')


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
            fabricate(root, 'XMapWindow', 'Xlib4', 1, with_data=True)
            result, manifest = self.run_select(root, 'XDestroyWindow', 'XInternAtom', 'XMapWindow', install=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            rows = json.loads(manifest.read_text())
            self.assertEqual(rows, [
                {'case': '/Xlib3/XDestroyWindow', 'purpose': 1},
                {'case': '/Xlib3/XDestroyWindow', 'purpose': 2},
                {'case': '/Xlib3/XDestroyWindow', 'purpose': 3},
                {'case': '/Xlib4/XInternAtom', 'purpose': 1},
                {'case': '/Xlib4/XInternAtom', 'purpose': 2},
                {'case': '/Xlib4/XMapWindow/XMapWindow', 'purpose': 1},
            ])
            scenario = (root / 'xts5/tet_scen').read_text()
            self.assertIn('\nselected-core\n\t"selected scenario selected-core: 3 cases"\n\t/Xlib3/XDestroyWindow\n\t/Xlib4/XInternAtom\n\t/Xlib4/XMapWindow/XMapWindow\n', scenario)
            self.assertTrue(scenario.startswith('all\n'), 'the suite\'s own scenarios stay first')
            # Installing again replaces the block rather than adding a second.
            again, _ = self.run_select(root, 'XDestroyWindow', install=True)
            self.assertEqual(again.returncode, 0, again.stderr)
            scenario = (root / 'xts5/tet_scen').read_text()
            self.assertEqual(scenario.count('\nselected-core\n'), 1)
            self.assertNotIn('/Xlib4/XInternAtom', scenario)
            self.assertIn('\t"selected scenario selected-core: 1 cases"\n\t/Xlib3/XDestroyWindow\n', scenario)

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
