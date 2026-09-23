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


def fabricate_typed(root, case, directory, test_types):
    """A case whose purposes each set one test type, as the Xproto sources do."""
    source = root / 'xts5' / directory / f'{case}.m'
    source.parent.mkdir(parents=True, exist_ok=True)
    body = f'>># {case}\n' + ''.join(
        f'>>ASSERTION Bad A\n>>STRATEGY\nSend it.\n>>CODE\n\ttest_type = {t};\n\ttestfunc(tester);\n'
        for t in test_types)
    source.write_text(body)


class SelectionTests(unittest.TestCase):
    def run_select(self, root, *cases, install=False, exclude=()):
        manifest = root / 'manifest.json'
        command = [sys.executable, '-B', str(HERE / 'xts_select.py'), '--xts-root', str(root),
                   '--scenario', 'selected-core', '--manifest', str(manifest)]
        for case in cases:
            command += ['--case', case]
        for name in exclude:
            command += ['--exclude-test-type', name]
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



    def test_an_excluded_test_type_leaves_its_purposes_out_by_name(self):
        # Every Xproto case ends in a TOO_LONG purpose that deadlocks the
        # connection (t165). Excluding it by its test type keeps the case,
        # narrows the scenario line to the purposes kept, and writes the
        # exclusions beside the manifest so the omission is on record.
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            fabricate(root, 'XInternAtom', 'Xlib4', 2)
            fabricate_typed(root, 'pAllocColor', 'Xproto', ['GOOD', 'BAD_LENGTH', 'TOO_LONG', 'GOOD'])
            fabricate_typed(root, 'pBell', 'Xproto', ['TOO_LONG'])
            result, manifest = self.run_select(root, 'XInternAtom', 'pAllocColor', install=True,
                                               exclude=['TOO_LONG'])
            self.assertEqual(result.returncode, 0, result.stderr)
            rows = json.loads(manifest.read_text())
            self.assertEqual(rows, [
                {'case': '/Xlib4/XInternAtom', 'purpose': 1},
                {'case': '/Xlib4/XInternAtom', 'purpose': 2},
                {'case': '/Xproto/pAllocColor', 'purpose': 1},
                {'case': '/Xproto/pAllocColor', 'purpose': 2},
                {'case': '/Xproto/pAllocColor', 'purpose': 4},
            ])
            excluded = json.loads((root / 'manifest.excluded.json').read_text())
            self.assertEqual(excluded, [{'case': '/Xproto/pAllocColor', 'purpose': 3, 'test_types': ['TOO_LONG']}])
            scenario = (root / 'xts5/tet_scen').read_text()
            self.assertIn('\t/Xlib4/XInternAtom\n\t/Xproto/pAllocColor{1-2,4}\n', scenario)
            self.assertEqual(json.loads(result.stdout)['excluded'], 1)
            # A case with nothing left is a selection error, not an empty line.
            result, _ = self.run_select(root, 'pBell', exclude=['TOO_LONG'])
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('every purpose', result.stderr)
            # A malformed test type is refused before anything is read.
            result, _ = self.run_select(root, 'pAllocColor', exclude=['too long'])
            self.assertNotEqual(result.returncode, 0)


if __name__ == '__main__':
    unittest.main()
