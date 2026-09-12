import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from check import read_profile, profile_commands
from run import HERE, profile_definition


class ProfileTests(unittest.TestCase):
    def test_manifests_match_real_implementations(self):
        for profile in ('core', 'xtest'):
            manifest, cases = profile_definition(profile)
            self.assertEqual(set(cases), {item['id'] for item in manifest['cases']})

    def test_native_does_not_fall_back_to_wire_or_default_host(self):
        commands = profile_commands('native-input', Path('/output'), Path('/target'))
        self.assertEqual(len(commands), 1)
        self.assertIn(str(HERE / 'native.py'), commands[0])

    def test_profile_exit_zero_without_complete_report_fails(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary)
            self.assertEqual(read_profile(output, 0)['status'], 'NORESULT')
            for report in ({}, {'status': 'PASS'}, {'status': 'PASS', 'required': 0,
                          'executed': 0, 'failures': []}, {'status': 'PASS', 'required': 2,
                          'executed': 1, 'failures': []}):
                (output / 'report.json').write_text(json.dumps(report))
                self.assertNotEqual(read_profile(output, 0)['status'], 'PASS')
            (output / 'report.json').write_text(json.dumps({'status': 'PASS', 'required': 2,
                                                            'executed': 2, 'failures': []}))
            self.assertEqual(read_profile(output, 0)['status'], 'PASS')
            self.assertEqual(read_profile(output, 124)['status'], 'FAIL')

    def test_xtest_direct_child_and_inside_refused_before_connections(self):
        for options in (['--child', '/fabricated/never-connect'], ['--inside']):
            result = subprocess.run([sys.executable, '-B', str(HERE / 'run.py'), '--profile', 'xtest',
                                     *options], capture_output=True, timeout=3)
            self.assertNotEqual(result.returncode, 0)
            self.assertNotIn(b'FileNotFoundError', result.stderr)
            self.assertNotIn(b'ConnectionRefusedError', result.stderr)
