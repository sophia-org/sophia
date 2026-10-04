import unittest
from damage_attribution import attribute, CAUSES, FRAME_FIELDS, PRESENT_FIELDS


def records():
    rows = []
    for t, n in ((1_000_000, 1), (6_000_000, 2)):
        render = dict.fromkeys(FRAME_FIELDS, 0)
        render.update(composition_full_frames_count=n, damage_full_plan_count=n,
                      damage_full_plan_coverage_count=n)
        causes = dict.fromkeys(CAUSES | {'full_' + c for c in CAUSES}, 0)
        causes.update(precise_surface=n, full_precise_surface=n)
        present = dict.fromkeys(PRESENT_FIELDS, 0)
        present.update(absent=n, source_pixels=100*n, rect_pixels=100*n, rects=n)
        for prefix, fields in (('sophia_live_render_work', render),
                               ('sophia_live_damage_causes', causes),
                               ('sophia_present_damage', present)):
            rows.append(prefix + f' schema=1 observed_monotonic_usec={t} ' +
                        ' '.join(f'{k}={v}' for k, v in sorted(fields.items())))
    return '\n'.join(rows)


class Attribution(unittest.TestCase):
    def test_interval_is_actual_counts_and_reports_its_coverage(self):
        result = attribute(records(), 0, 10_000_000)
        self.assertEqual(result['status'], 'VALID', result)
        self.assertEqual(result['counter_coverage'], .5)
        self.assertEqual(result['frames'], {'full': 1, 'partial': 0})
        self.assertEqual(result['causes']['full_precise_surface'], 1)
        self.assertEqual(result['present']['source_pixels'], 100)

    def test_missing_reset_wrong_subreasons_and_misaligned_records_fail(self):
        for old, new in [('absent=2', 'absent=0'),
                         ('source_pixels=200', 'wrong=200'),
                         ('damage_full_plan_coverage_count=2', 'damage_full_plan_coverage_count=3'),
                         ('full_precise_surface=2', 'full_precise_surface=3'),
                         ('sophia_present_damage schema=1 observed_monotonic_usec=6000000',
                          'sophia_present_damage schema=1 observed_monotonic_usec=6000001')]:
            self.assertEqual(attribute(records().replace(old, new), 0, 10_000_000)['status'],
                             'INVALID', old)

    def test_frame_partitions_empty_progress_and_present_counts_fail_closed(self):
        for old, new in [('damage_full_plan_count=2', 'damage_full_plan_count=3'),
                         ('precise_surface=2', 'precise_surface=3'),
                         ('composition_full_frames_count=2', 'composition_full_frames_count=1'),
                         ('rects=2', 'rects=1')]:
            self.assertEqual(attribute(records().replace(old, new), 0, 10_000_000)['status'],
                             'INVALID', old)
        self.assertEqual(attribute(records(), 10, 10)['status'], 'INVALID')

    def test_unavailable_reduction_is_a_reason_bucket_not_a_mask_assertion(self):
        text = records()
        for n in (1, 2):
            text = text.replace(f'damage_full_plan_count={n}', 'damage_full_plan_count=0')
            text = text.replace(f'damage_full_plan_coverage_count={n}', 'damage_full_plan_coverage_count=0')
            text = text.replace('damage_full_damage_unavailable_count=0',
                                f'damage_full_damage_unavailable_count={n}', 1)
        result = attribute(text, 0, 10_000_000)
        self.assertEqual(result['status'], 'VALID', result)
        self.assertEqual(result['full_without_usable_reduction'], 1)
        self.assertEqual(result['causes']['full_precise_surface'], 1)

    def test_overlap_is_not_mistaken_for_extra_frames_or_union_area(self):
        rows = records().splitlines()
        for i, row in enumerate(rows):
            if row.startswith('sophia_live_damage_causes ') and 'observed_monotonic_usec=6000000' in row:
                rows[i] = row.replace(' full_geometry=0 ', ' full_geometry=1 ').replace(' geometry=0 ', ' geometry=1 ')
        text = '\n'.join(rows).replace('rect_pixels=200', 'rect_pixels=400')
        result = attribute(text, 0, 10_000_000)
        self.assertEqual(result['status'], 'VALID', result)
        self.assertEqual(result['causes']['full_geometry'] + result['causes']['full_precise_surface'], 2)
        self.assertGreater(result['present']['rect_pixels'], result['present']['source_pixels'])

if __name__ == '__main__':
    unittest.main()
