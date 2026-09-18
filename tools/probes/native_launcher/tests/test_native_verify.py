import sys
from pathlib import Path
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from verify import Invalid, verify
from profile import profile


def transcript():
    lines = [
        'sophia_live_wm_configuration schema=2 status=committed',
        'sophia_shell_component schema=1 status=negotiated slot=0 role=bar connection_epoch=1 content_grant_epoch=1 revision=6 gpu_mode=direct gpu_grant_epoch=1 device_major=226 device_minor=129',
        'sophia_shell_component_catalog schema=1 status=built generation=1 entries=12',
        'sophia_shell_component schema=1 status=negotiated slot=1 role=application_launcher connection_epoch=2 content_grant_epoch=2 revision=7 gpu_mode=denied gpu_grant_epoch=0 device_major=0 device_minor=0',
    ]
    for epoch in (1, 2):
        grant = f'connection_epoch={epoch} content_grant_epoch={epoch}'
        lines.append(f'sophia_live_shell_content schema=1 status=outputs facts_generation=1 outputs=2 {grant}')
        for output in (1, 2):
            for candidate in (output * 2 - 1, output * 2):
                lines.append(f'sophia_live_shell_content schema=1 status=presented output={output} candidate_generation={candidate} presentation_epoch={candidate} {grant}')
    lines += ['sophia_native_launcher schema=1 status=process_started transaction=42',
              'sophia_shell_components_shutdown schema=1 status=quiescent']
    return lines


class EvidenceTests(unittest.TestCase):
    def test_positive_plain_and_structured(self):
        lines = transcript()
        self.assertEqual(verify('\n'.join(lines))['status'], 'pass')
        self.assertEqual(verify('\n'.join(f'{i}\t1\t1\t{line}' for i, line in enumerate(lines)))['status'], 'pass')

    def test_every_required_record_missing_refuses(self):
        lines = transcript()
        for index in (0, 1, 2, 3, 4, len(lines)-2, len(lines)-1):
            with self.subTest(index=index), self.assertRaises(Invalid):
                verify('\n'.join(lines[:index] + lines[index+1:]))

    def test_component_fields_are_mandatory_unique_bounded(self):
        lines = transcript()
        for field in lines[1].split()[1:]:
            with self.subTest(field=field), self.assertRaises(Invalid):
                variant = lines.copy(); variant[1] = variant[1].replace(' ' + field, '', 1)
                verify('\n'.join(variant))
        for field in ('slot=2', 'revision=65536', 'device_major=4294967296',
                      'device_minor=-1', 'gpu_grant_epoch=0', 'gpu_mode=denied',
                      'connection_epoch=0', 'content_grant_epoch=18446744073709551616'):
            variant = lines.copy(); key = field.split('=')[0]
            variant[1] = ' '.join(field if v.startswith(key+'=') else v for v in variant[1].split())
            with self.subTest(field=field), self.assertRaises(Invalid):
                verify('\n'.join(variant))
        with self.assertRaises(Invalid):
            verify('\n'.join(lines).replace('slot=0', 'slot=0 slot=0'))
        with self.assertRaises(Invalid):
            verify('\n'.join(lines).replace('device_major=226', 'device_major=' + '1' * 5000))

    def test_restart_alias_or_launcher_gpu_refuses(self):
        lines = transcript()
        variants = [lines[:2]+[lines[1]]+lines[2:]]
        for before, after in [('slot=1', 'slot=0'), ('connection_epoch=2', 'connection_epoch=1'),
                              ('content_grant_epoch=2', 'content_grant_epoch=1'),
                              ('gpu_mode=denied', 'gpu_mode=direct'), ('revision=7', 'revision=6')]:
            variant = lines.copy(); variant[3] = variant[3].replace(before, after); variants.append(variant)
        for variant in variants:
            with self.assertRaises(Invalid): verify('\n'.join(variant))

    def test_cannot_borrow_bar_presentations_for_launcher(self):
        lines = transcript()
        for variant in [
            [v for v in lines if not ('status=presented' in v and 'connection_epoch=2' in v)],
            [v for v in lines if not ('status=presented output=2' in v and 'connection_epoch=2' in v)],
            [v.replace('connection_epoch=2', 'connection_epoch=3') if 'status=presented' in v else v for v in lines],
        ]:
            with self.assertRaises(Invalid): verify('\n'.join(variant))

    def test_failure_empty_catalog_and_missing_second_generation_refuse(self):
        text = '\n'.join(transcript())
        for bad in [text.replace('entries=12', 'entries=0'),
                    text.replace('candidate_generation=4', 'candidate_generation=3'),
                    text.replace('status=quiescent', 'status=retained'),
                    text + '\nsophia_shell_component schema=1 status=start_failed',
                    text + '\nsophia_native_launcher schema=1 status=execution_rejected',
                    text + '\nsophia_live_session schema=1 failure_code=unclassified']:
            with self.assertRaises(Invalid): verify(bad)

    def test_profile_explicit_roles_no_bindings_or_autostart(self):
        result = profile('/opt/lom', '/private/a "quoted".kdl', '/opt/bemenu-sophia')
        self.assertIn('shell-component "panel" "bar"', result)
        self.assertIn('shell-component "menu" "application-launcher"', result)
        self.assertIn('config "/private/a \\"quoted\\".kdl"', result)
        self.assertIn('    startup\n', result)
        self.assertNotIn('bind ', result)
        self.assertNotIn('shortcut', result)
        for bad in ('relative', '/path\nnewline', '/path\0nul'):
            with self.assertRaises(ValueError): profile(bad, '/config', '/bemenu')
