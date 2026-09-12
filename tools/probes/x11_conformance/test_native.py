import copy
import unittest

from native import evaluate, test_command, verdict


class NativeAccountingTests(unittest.TestCase):
    def test_integration_target_is_explicit_and_name_not_prefixed(self):
        command = test_command({'package': 'sophia-input-authority',
                                'target': {'kind': 'test', 'name': 'ledger'},
                                'test': 'a_physical_hold_survives'})
        self.assertEqual(command[5:8], ['--test', 'ledger', 'a_physical_hold_survives'])
        self.assertNotIn('--lib', command)

    def test_only_exact_execution_passes(self):
        self.assertEqual(verdict(0, 'test native::overlap ... ok\n'
            'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 8 filtered out; '
            'finished in 0.00s\n', 'native::overlap')['status'], 'PASS')

    def test_zero_match_skip_empty_and_other_test_are_not_evidence(self):
        for output in ('', 'PASS', 'test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; '
                       '8 filtered out; finished in 0.00s\n',
                       'test native::overlap ... ignored\n',
                       'test other::overlap ... ok\n'):
            with self.subTest(output=output):
                self.assertEqual(verdict(0, output, 'native::overlap')['status'], 'NORESULT')

    def test_deadline_and_exit_failure_cannot_be_pass(self):
        self.assertEqual(verdict(124, 'PASS', 'test')['status'], 'TIMEOUT')
        self.assertEqual(verdict(1, 'PASS', 'test')['status'], 'FAIL')

    def test_all_required_results_are_accounted(self):
        manifest = {'obligations': [{'id': 'overlap'}, {'id': 'revoke'}]}
        results = [{'case': name, 'status': 'PASS'} for name in ('overlap', 'revoke')]
        self.assertEqual(evaluate(manifest, results)['status'], 'PASS')
        for damaged in ([], results[:1], results + results[:1],
                        results + [{'case': 'foreign', 'status': 'PASS'}]):
            self.assertEqual(evaluate(manifest, damaged)['status'], 'FAIL')
        for status in ('NORESULT', 'TIMEOUT', 'FAIL', 'SKIP', 'UNTESTED', 'UNSUPPORTED'):
            damaged = copy.deepcopy(results)
            damaged[0]['status'] = status
            self.assertEqual(evaluate(manifest, damaged)['status'], 'FAIL')
