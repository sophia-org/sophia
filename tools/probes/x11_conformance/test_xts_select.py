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


def fabricate_with_inclusions(root, case, directory):
    """A case as the graphics sections write them: purposes of its own, a
    `>>ASSERTION gc` naming components (one commented out), and a bare
    include beside the source. The preprocessor counts six purposes here."""
    source = root / 'xts5' / directory / case / f'{case}.m'
    source.parent.mkdir(parents=True, exist_ok=True)
    source.write_text(f'>># {case}\n'
                      '>>ASSERTION Good A\n>>CODE\nown1();\n'
                      '>>ASSERTION gc\nOn a call the GC components\n.M function ,\n'
                      '>># .M join-style ,\n.M tile ,\nare used.\n'
                      '>>INCLUDE extra.mc\n'
                      '>>ASSERTION Bad A\n>>CODE\n\ttest_type = TOO_LONG;\n\town2();\n')
    (source.parent / 'extra.mc').write_text('>>ASSERTION Good A\n>>CODE\nextra();\n')
    gc = root / 'xts5' / 'lib' / 'gc'
    gc.mkdir(parents=True, exist_ok=True)
    (gc / 'function.mc').write_text('>>ASSERTION Good A\n>>CODE\nf1();\n'
                                    '>>ASSERTION Bad A\n>>CODE\n\ttest_type = TOO_LONG;\n\tf2();\n')
    (gc / 'tile.mc').write_text('>>ASSERTION Good A\n>>CODE\nt1();\n')
    (gc / 'join-styl.mc').write_text('>>ASSERTION Good A\n>>CODE\nnever();\n')
    scenario_file = root / 'xts5' / 'tet_scen'
    if not scenario_file.exists():
        scenario_file.write_text('all\n\t"everything"\n')


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

    def test_purposes_are_counted_over_the_preprocessors_inclusions(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            fabricate_with_inclusions(root, 'XDrawThing', 'Xlib9')
            result, manifest = self.run_select(root, 'XDrawThing')
            self.assertEqual(result.returncode, 0, result.stderr)
            rows = json.loads(manifest.read_text())
            # own1, function 1 and 2, tile 1, extra, own2: the gc line is no
            # purpose, the commented component is not included.
            self.assertEqual([r['purpose'] for r in rows], [1, 2, 3, 4, 5, 6])
            result, manifest = self.run_select(root, 'XDrawThing', exclude=('TOO_LONG',))
            self.assertEqual(result.returncode, 0, result.stderr)
            rows = json.loads(manifest.read_text())
            # Numbered where the preprocessor puts them: function's second
            # purpose is 3 and the case's own last is 6.
            self.assertEqual([r['purpose'] for r in rows], [1, 2, 4, 5])
            excluded = json.loads((root / 'manifest.excluded.json').read_text())
            self.assertEqual([r['purpose'] for r in excluded], [3, 6])

    def test_a_missing_include_or_unknown_component_is_refused(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            fabricate_with_inclusions(root, 'XDrawThing', 'Xlib9')
            (root / 'xts5' / 'Xlib9' / 'XDrawThing' / 'extra.mc').unlink()
            result, _ = self.run_select(root, 'XDrawThing')
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('>>INCLUDE extra.mc', result.stderr)

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


class DeclarationTests(unittest.TestCase):
    """xts_declare.py writes what a journal observed, with a reason for each,
    or nothing."""

    journal = ('10|0 /Xproto/pBell 00:00|TC Start\n'
               '200|0 1 00:00|TP Start\n220|0 1 0 00:00|PASS\n'
               '200|0 2 00:00|TP Start\n220|0 2 5 00:00|UNTESTED\n'
               '10|1 /Xproto/pKillClient 00:00|TC Start\n'
               '200|1 1 00:00|TP Start\n220|1 1 1 00:00|FAIL\n')
    manifest = [{'case': '/Xproto/pBell', 'purpose': 1}, {'case': '/Xproto/pBell', 'purpose': 2},
                {'case': '/Xproto/pKillClient', 'purpose': 1}]

    def declare(self, reasons, journal=None):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / 'manifest.json').write_text(json.dumps(self.manifest))
            (root / 'journal').write_text(journal or self.journal)
            (root / 'reasons.json').write_text(json.dumps(reasons))
            result = subprocess.run([sys.executable, '-B', str(HERE / 'xts_declare.py'),
                                     '--manifest', str(root / 'manifest.json'), '--journal', str(root / 'journal'),
                                     '--reasons', str(root / 'reasons.json'), '--output', str(root / 'out.json')],
                                    capture_output=True, text=True)
            rows = json.loads((root / 'out.json').read_text()) if (root / 'out.json').exists() else None
            return result, rows

    def test_observed_dispositions_are_declared_with_their_reasons(self):
        result, rows = self.declare({'/Xproto/pBell#2': 'cannot be shortened', '/Xproto/pKillClient': 't166'})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(rows, [
            {'case': '/Xproto/pBell', 'purpose': 1},
            {'case': '/Xproto/pBell', 'purpose': 2, 'expected': 'UNTESTED', 'reason': 'cannot be shortened'},
            {'case': '/Xproto/pKillClient', 'purpose': 1, 'expected': 'FAIL', 'reason': 't166'},
        ])
        self.assertEqual(json.loads(result.stdout), {'purposes': 3, 'declared': 2})

    def test_a_disposition_without_a_reviewed_reason_is_refused(self):
        result, rows = self.declare({'/Xproto/pBell#2': 'cannot be shortened'})
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('/Xproto/pKillClient#1: FAIL', result.stderr)
        self.assertIsNone(rows)

    def test_a_run_level_disposition_cannot_be_declared(self):
        # A purpose that never started describes the run; the answer is to
        # rerun, not to write MISSING into the manifest.
        journal = self.journal.replace('10|1 /Xproto/pKillClient 00:00|TC Start\n'
                                       '200|1 1 00:00|TP Start\n220|1 1 1 00:00|FAIL\n', '')
        result, rows = self.declare({'/Xproto/pBell#2': 'x', '/Xproto/pKillClient': 'x'}, journal)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('describes the run', result.stderr)


if __name__ == '__main__':
    unittest.main()
