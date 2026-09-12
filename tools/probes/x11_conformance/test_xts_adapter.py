"""Fabricated TET plumbing tests, never reported as an executed XTS suite."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

import isolation
from run import HERE, isolated_case


class AdapterTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        try:
            result = isolation.launch(['/usr/bin/true'], timeout=5)
        except isolation.IsolationError as error:
            raise unittest.SkipTest(str(error)) from error
        if result.returncode:
            raise unittest.SkipTest('BLOCKED: ' + result.stderr.decode(errors='replace'))

    def invoke(self, verdict='PASS', process_status=0, omit=False):
        with tempfile.TemporaryDirectory(prefix='sophia-fabricated-tet-') as temporary:
            work = Path(temporary)
            root = work / 'fixture'
            (root / 'xts5').mkdir(parents=True)
            (root / 'xts5/tcc').write_text('#!/bin/sh\nexit 0\n')
            (root / 'xts5/tcc').chmod(0o700)
            journal = '10|1 /fabricated/case 0|Start\n'
            if not omit:
                journal += '200|1 1 0|Start\n'
                journal += f'220|1 1 {0 if verdict == "PASS" else 7} 0|{verdict}\n'
            # The separate fake check.sh writes only an explicitly fabricated
            # purpose. It does not claim to speak X or validate Sophia.
            (root / 'check.sh').write_text('mkdir -p results/fresh\n'
                "cat >results/fresh/journal <<'JOURNAL'\n" + journal +
                f'JOURNAL\nexit {process_status}\n')
            host = work / 'host'
            host.write_text('#!/usr/bin/python3\nimport socket,sys,time\n'
                            's=socket.socket(socket.AF_UNIX)\ns.bind(sys.argv[1])\n'
                            's.listen()\ntime.sleep(60)\n')
            host.chmod(0o700)
            expected = work / 'expected.json'
            expected.write_text('[{"case":"/fabricated/case","purpose":1}]')
            output = work / 'output'
            command = [sys.executable, '-B', str(HERE / 'xts.py'), '--host', str(host),
                       '--xts-root', str(root), '--scenario', 'fabricated', '--expected',
                       str(expected), '--output', str(output), '--timeout', '5']
            result = subprocess.run(command, capture_output=True, timeout=25)
            report = json.loads((output / 'report.json').read_text())
            return result.returncode, report, result.stderr

    def test_fabricated_journal_positive_control(self):
        code, report, stderr = self.invoke()
        self.assertEqual(code, 0, (report, stderr))
        self.assertEqual(report['status'], 'PASS')

    def test_private_xtest_entry_reaches_only_owned_host(self):
        with tempfile.TemporaryDirectory() as temporary:
            result = isolated_case(Path('/usr/bin/false'), 'xtest_discovery', 'little', 2,
                                   Path(temporary) / 'host.log')
        self.assertEqual(result['status'], 'FAIL')
        self.assertEqual(result['detail'], 'host exited 1 before bind')

    def test_noresult_and_missing_purpose_fail(self):
        for kwargs in ({'verdict': 'NORESULT'}, {'omit': True}):
            with self.subTest(kwargs=kwargs):
                code, report, _ = self.invoke(**kwargs)
                self.assertNotEqual(code, 0)
                self.assertEqual(report['status'], 'FAIL')

    def test_partial_success_cannot_override_process_failure(self):
        code, report, _ = self.invoke(process_status=1)
        self.assertNotEqual(code, 0)
        self.assertEqual(report['status'], 'FAIL')


if __name__ == '__main__':
    suite = unittest.defaultTestLoader.loadTestsFromModule(__import__(__name__))
    result = unittest.TextTestRunner(verbosity=2).run(suite)
    status = 'FAIL' if not result.wasSuccessful() else 'BLOCKED' if result.skipped else 'PASS'
    print(json.dumps({'status': status, 'tests_run': result.testsRun,
                      'evidence_kind': 'fabricated_adapter_regression', 'xts_suite_executed': False}))
    raise SystemExit(1 if not result.wasSuccessful() else 2 if result.skipped else 0)
