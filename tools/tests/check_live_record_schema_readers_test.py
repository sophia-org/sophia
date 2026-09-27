"""Mutation checks for schema drift, separate from production evidence readers."""
import importlib.util
from pathlib import Path
import unittest
from unittest.mock import patch
import sys

sys.dont_write_bytecode = True

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('schema_readers', ROOT / 'tools/check_live_record_schema_readers.py')
guard = importlib.util.module_from_spec(spec)
spec.loader.exec_module(guard)


class SchemaReaderChecks(unittest.TestCase):
    def setUp(self):
        self.sources = {'emitter.rs': '''
            "sophia_live_native_resources schema=12 status=complete count={}",
            "sophia_live_rendering_efficiency schema=2 status=complete count={}",
            "sophia_live_wm schema=4 status=ready adapter=sophia_wm_v1",
            "sophia_live_session schema={} status=bounded_complete elapsed={}",
            if startup_proof_requested { 16 } else { 17 },
        '''}
        self.readers = {}
        for name in guard.PROOF_READERS:
            self.readers['tools/' + name] = 'sophia_live_session schema=16 status=bounded_complete '
        for name in guard.NORMAL_READERS:
            self.readers['tools/' + name] = 'sophia_live_session schema=(16|17) status=bounded_complete '
        for name in guard.ARCHIVE_READERS:
            self.readers['tools/' + name] = '# Archive-only: historical proof\nsophia_live_session schema=13 status=bounded_complete '
        for name in guard.PARSED_READERS:
            expression = '[[ "${observed[schema]}" =~ ^(14|16)$ ]]'
            if name == 'verify_qemu_session_evidence.sh':
                expression = 'if [[ ! "$completion_schema" =~ ^(14|16)$ ]]; then'
            self.readers['tools/' + name] = 'sophia_live_session .*status=bounded_complete \n' + expression
        for name in guard.FIELD_ONLY_READERS:
            self.readers['tools/' + name] = 'sophia_live_session .*status=bounded_complete '
        for name in guard.WM_READERS:
            self.readers['tools/' + name] += '\nsophia_live_wm schema=4 status=ready '
        self.readers[guard.RUST_PROOF_READER] = 'last_record(&text, "sophia_live_session schema=16 ")'

    def failures(self):
        versions, completion = guard.emitter_schemas(self.sources)
        return guard.check_readers(self.readers, versions, completion)[1]

    def test_current_tree(self):
        _, failures = guard.audit(ROOT)
        self.assertEqual(failures, [])

    def test_reviewed_purposes(self):
        self.assertEqual(self.failures(), [])

    def test_conditional_bumps_reach_the_right_readers(self):
        # Model a direct normal-completion reader without keeping a product
        # script in Sophia solely to exercise the inventory classifier.
        with patch.object(guard, 'NORMAL_READERS', (*guard.NORMAL_READERS, 'fixture-normal.sh')):
            self.readers['tools/fixture-normal.sh'] = 'sophia_live_session schema=(16|17) status=bounded_complete '
            self.sources['emitter.rs'] = self.sources['emitter.rs'].replace('{ 17 }', '{ 18 }')
            failures = self.failures()
            self.assertTrue(any('fixture-normal.sh' in f for f in failures))
            self.assertFalse(any('verify_live_session_persistent_evidence.sh' in f for f in failures))
            self.sources['emitter.rs'] = self.sources['emitter.rs'].replace('{ 16 }', '{ 19 }')
            self.assertTrue(any('verify_live_session_persistent_evidence.sh' in f for f in self.failures()))

    def test_proof_reader_cannot_be_relaxed_to_normal_completion(self):
        name = guard.RUST_PROOF_READER
        self.readers[name] = self.readers[name].replace('schema=16', 'schema=(16|17)')
        self.assertTrue(any('unrequested-proof schema' in f for f in self.failures()))

    def test_unresolved_or_ambiguous_emitter_fails(self):
        source = self.sources['emitter.rs']
        for changed in [source.replace('{ 17 }', '{ normal_schema }'), source + source,
                        source.replace('schema={} status=bounded_complete', 'schema=16 status=bounded_complete')]:
            with self.subTest(changed=changed):
                with self.assertRaises(ValueError):
                    guard.emitter_schemas({'emitter.rs': changed})

    def test_unrelated_status_does_not_change_readiness(self):
        self.sources['other.rs'] = '"sophia_live_wm schema=99 status=session_action_committed"'
        self.assertEqual(self.failures(), [])
        self.sources['other.rs'] = '"sophia_live_wm schema=99 status=ready "'
        with self.assertRaisesRegex(ValueError, 'disagree'):
            self.failures()

    def test_stale_native_readiness_is_detected_despite_generic_selector(self):
        name = 'tools/fixture-wm.sh'
        with patch.object(guard, 'WM_READERS', ('fixture-wm.sh',)):
            self.readers[name] = 'sophia_live_wm schema=1 status=ready \nsophia_live_wm schema=[0-9]+ status=ready '
            self.assertTrue(any('no readiness branch' in f for f in self.failures()))

    def test_every_direct_acceptance_site_is_checked(self):
        self.readers[guard.RUST_PROOF_READER] += '\nsophia_live_session schema=15 status=bounded_complete '
        self.assertTrue(any('schema=15' in f for f in self.failures()))

    def test_parsed_acceptance_and_its_disappearance_are_checked(self):
        name = 'tools/verify_live_session_milestone4_evidence.sh'
        old = self.readers[name]
        self.readers[name] = old.replace('(14|16)', '(14)')
        self.assertTrue(any('cannot read' in f for f in self.failures()))
        self.readers[name] = old.replace('observed[schema]', 'renamed[schema]')
        self.assertTrue(any('parsed schema check' in f for f in self.failures()))

    def test_new_reader_and_disappeared_reader_require_review(self):
        self.readers['tools/new.sh'] = 'sophia_live_session schema=16 status=bounded_complete '
        self.assertTrue(any('no reviewed purpose' in f for f in self.failures()))
        del self.readers['tools/new.sh']
        del self.readers['tools/verify_live_session_persistent_evidence.sh']
        self.assertTrue(any('disappeared' in f for f in self.failures()))

    def test_archive_exception_is_narrow_and_explicit(self):
        name = 'tools/verify_live_session_two_xterm_evidence.sh'
        self.readers[name] = self.readers[name].replace('# Archive-only:', '# Ordinary:')
        self.assertTrue(any('archive-only declaration' in f for f in self.failures()))

    def test_field_only_reader_cannot_acquire_a_hidden_schema_filter(self):
        self.readers['tools/verify_qemu_emergency_recovery_evidence.sh'] += '\nsophia_live_session schema=15 status=bounded_complete '
        self.assertTrue(any('acquired a schema restriction' in f for f in self.failures()))

    def test_existing_resource_guard_still_catches_drift(self):
        self.readers['tools/resources.sh'] = 'sophia_live_native_resources schema=(10|11) status=complete '
        self.assertTrue(any('resources.sh' in f for f in self.failures()))
        self.readers['tools/resources.sh'] = self.readers['tools/resources.sh'].replace('(10|11)', '(10|11|12)')
        self.assertEqual(self.failures(), [])


if __name__ == '__main__':
    unittest.main()
