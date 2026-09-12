"""Regressions for the gate itself, including the demonstrated yserver false positives."""
import copy
import json
from pathlib import Path
import subprocess
import sys
import unittest

from report import evaluate, load_manifest
from run import bounded, clean_environment, decode_result
from xts_report import evaluate_journal

HERE = Path(__file__).resolve().parent


class GateTests(unittest.TestCase):
    def setUp(self):
        self.manifest = load_manifest(HERE / 'manifest.json')
        self.results = [{'case': c['id'], 'byte_order': order, 'status': 'PASS'}
                        for c in self.manifest['cases'] for order in self.manifest['byte_orders']]

    def test_complete_pass(self):
        self.assertEqual(evaluate(self.manifest, self.results)['status'], 'PASS')

    def test_missing_and_empty_are_failures(self):
        for result in ([], self.results[:-1]):
            self.assertEqual(evaluate(self.manifest, result)['status'], 'FAIL')

    def test_nonpassing_verdicts_are_failures(self):
        for status in ('NORESULT', 'TIMEOUT', 'UNSUPPORTED', 'UNTESTED', 'NOTINUSE', 'FAIL'):
            with self.subTest(status=status):
                results = copy.deepcopy(self.results)
                results[0]['status'] = status
                self.assertEqual(evaluate(self.manifest, results)['status'], 'FAIL')

    def test_duplicate_and_foreign_results_fail(self):
        self.assertEqual(evaluate(self.manifest, self.results + [self.results[0]])['status'], 'FAIL')
        self.assertEqual(evaluate(self.manifest, self.results +
                                 [{'case': 'invented', 'byte_order': 'little', 'status': 'PASS'}])['status'], 'FAIL')

    def test_manifest_unimplemented_obligation_fails(self):
        manifest = copy.deepcopy(self.manifest)
        manifest['cases'].append({'id': 'not_executed', 'mandatory': True})
        self.assertEqual(evaluate(manifest, self.results)['status'], 'FAIL')

    def test_absolute_process_timeout_even_with_output(self):
        status, _, _ = bounded([sys.executable, '-c',
                               'import time\nwhile True:\n print("progress",flush=True);time.sleep(.01)'],
                              .15, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        self.assertEqual(status, 124)

    def test_clear_live_opt_ins(self):
        from unittest.mock import patch
        with patch.dict('os.environ', {'SOPHIA_LIVE_NATIVE_SCANOUT': '1', 'HAGIA_X': 'x',
                                      'DISPLAY': ':0', 'WAYLAND_SOCKET': '7', 'PYTHONOPTIMIZE': '1'}):
            env = clean_environment()
            self.assertFalse(any(k.startswith(('SOPHIA_', 'HAGIA_')) for k in env))
            self.assertNotIn('DISPLAY', env)
            self.assertNotIn('WAYLAND_SOCKET', env)
            self.assertNotIn('PYTHONOPTIMIZE', env)

    def test_client_output_is_not_an_exit_code_only_gate(self):
        for stdout in (b'', b'{}', b'null', b'{"status":"NORESULT"}', b'PASS'):
            self.assertEqual(decode_result(0, stdout, b'')['status'], 'NORESULT')
        self.assertEqual(decode_result(1, b'{"status":"PASS"}', b'')['status'], 'FAIL')
        self.assertEqual(decode_result(124, b'{"status":"PASS"}', b'')['status'], 'TIMEOUT')

    def test_optimized_python_cannot_disable_assertions(self):
        result = subprocess.run([sys.executable, '-O', '-B', str(HERE / 'run.py'), '--help'],
                                capture_output=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(b'without -O', result.stderr)

    def test_extension_manifest_missing_case_rejected(self):
        manifest = copy.deepcopy(self.manifest)
        manifest['extensions']['SHAPE']['mandatory_cases'].append('missing')
        with self.assertRaises(AssertionError):
            evaluate(manifest, self.results)

    def test_xts_namespace_hides_live_sockets_and_devices(self):
        from isolation import command as isolated_command
        from xts import inner_command
        command = isolated_command('/usr/bin/bwrap', inner_command(), [], 9)
        self.assertIn('--unshare-all', command)
        self.assertIn('--tmpfs', command)
        self.assertIn('--dev', command)
        self.assertIn('--activation-fd', command)
        self.assertNotIn(['/', '/'], [command[i+1:i+3] for i, x in enumerate(command)
                                     if x == '--ro-bind'])
        self.assertNotIn('--share-net', command)

    def test_xts_direct_inner_entry_cannot_create_host(self):
        from unittest.mock import patch
        from isolation import IsolationError
        from xts import inside
        with patch('xts.subprocess.Popen') as spawn:
            with self.assertRaises(IsolationError):
                inside(Path('/fabricated/host'), -1)
            spawn.assert_not_called()

    def test_missing_xts_is_a_dependency_blocker(self):
        from unittest.mock import patch
        from xts import dependency_errors
        with patch('shutil.which', return_value=None):
            failures = dependency_errors(Path('/nonexistent-sophia-xts'), None)
        self.assertEqual(len(failures), 4)

    def test_new_accepted_request_cannot_disappear_from_inventory(self):
        from inventory import check_inventory
        root = HERE.parents[2]
        self.assertGreater(check_inventory(root, self.manifest)['declared_core_requests'], 50)
        manifest = copy.deepcopy(self.manifest)
        del manifest['core_request_inventory']['1']
        with self.assertRaises(ValueError):
            check_inventory(root, manifest)


class JournalTests(unittest.TestCase):
    expected = [{'case': '/Xlib3/XDestroyWindow', 'purpose': 1},
                {'case': '/Xlib3/XDestroyWindow', 'purpose': 2}]

    def journal(self, status='PASS', code=0, second=True):
        data = f'10|0 /Xlib3/XDestroyWindow 00:00|TC Start\n200|0 1 00:00|TP Start\n220|0 1 {code} 00:00|{status}\n'
        if second:
            data += '200|0 2 00:00|TP Start\n220|0 2 0 00:00|PASS\n'
        return data

    def test_selected_pass(self):
        self.assertEqual(evaluate_journal(self.expected, self.journal())['status'], 'PASS')

    def test_pass_to_noresult_is_failure(self):
        self.assertEqual(evaluate_journal(self.expected, self.journal('NORESULT', 7))['status'], 'FAIL')

    def test_missing_candidate_purpose_is_failure(self):
        self.assertEqual(evaluate_journal(self.expected, self.journal(second=False))['status'], 'FAIL')

    def test_started_without_result_is_failure(self):
        text = self.journal(second=False) + '200|0 2 00:00|TP Start\n'
        self.assertEqual(evaluate_journal(self.expected, text)['status'], 'FAIL')

    def test_timeout_with_all_passes_still_fails(self):
        self.assertEqual(evaluate_journal(self.expected, self.journal(), 124)['status'], 'FAIL')

    def test_no_unsupported_untested_or_notinuse_pass(self):
        for code, status in [(3, 'NOTINUSE'), (4, 'UNSUPPORTED'), (5, 'UNTESTED')]:
            self.assertEqual(evaluate_journal(self.expected, self.journal(status, code))['status'], 'FAIL')

    def test_empty_duplicate_forged_and_unstarted_results_rejected(self):
        for text in ['', self.journal() + '220|0 1 0 00:00|PASS\n',
                     self.journal('PASS', 7), '220|0 1 0 00:00|PASS\n']:
            with self.subTest(text=text), self.assertRaises(ValueError):
                evaluate_journal(self.expected, text)


if __name__ == '__main__':
    unittest.main()
