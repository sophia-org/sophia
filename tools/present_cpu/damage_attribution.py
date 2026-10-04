#!/usr/bin/env python3
"""Report one unchanged native-owner interval; never infer a smaller repaint."""
import argparse
import json
from pathlib import Path
from analyze import interval, FULL_REASONS

CAUSES = set('new_output output_changed compositor order geometry sampling generation '
             'missing_identity no_matching_transition invalid_transition origin rect_limit '
             'precision_restricted coordinate_overflow terminal_identity history_limit '
             'rebased precise_surface preview_identity cursor'.split())
SUBREASONS = {'damage_full_plan_' + name + '_count'
              for name in ('unspecified', 'capacity', 'rect_limit', 'coverage')}
PRESENT_FIELDS = set('absent explicit_full_rect explicit_regions effective_empty '
                     'source_pixels rect_pixels rects'.split())
FRAME_FIELDS = FULL_REASONS | SUBREASONS | {
    'composition_full_frames_count', 'composition_partial_frames_count',
    'composition_repaint_pixels_count', 'composition_target_pixels_count',
}


def attribute(text, start, end):
    try:
        if end <= start:
            raise ValueError('empty interval')
        render, span = interval(text, 'sophia_live_render_work', start, end, FRAME_FIELDS)
        causes, _ = interval(text, 'sophia_live_damage_causes', start, end,
                                     CAUSES | {'full_' + cause for cause in CAUSES})
        present, _ = interval(text, 'sophia_present_damage', start, end, PRESENT_FIELDS)
        # All three records are emitted together. Equal durations alone would
        # not prove matching endpoints; pin their actual observation times too.
        times = []
        for prefix in ('sophia_live_render_work', 'sophia_live_damage_causes', 'sophia_present_damage'):
            ts = []
            for line in text.splitlines():
                if line.startswith(prefix + ' '):
                    fields = dict(word.split('=', 1) for word in line.split()[1:] if '=' in word)
                    t = int(fields['observed_monotonic_usec'])
                    if start <= t <= end:
                        ts.append(t)
            times.append(ts)
        if times[0] != times[1] or times[0] != times[2]:
            raise ValueError('damage records have different observation times')
        full = render['composition_full_frames_count']
        partial = render['composition_partial_frames_count']
        if full + partial == 0:
            raise ValueError('no composed frames')
        if sum(render[k] for k in FULL_REASONS) != full:
            raise ValueError('full-repaint reasons do not partition frames')
        if sum(render[k] for k in SUBREASONS) != render['damage_full_plan_count']:
            raise ValueError('plan subreasons do not partition full_plan')
        for cause in CAUSES:
            if not 0 <= causes['full_' + cause] <= min(causes[cause], full):
                raise ValueError('full cause exceeds its frame cohort: ' + cause)
            if causes[cause] > full + partial:
                raise ValueError('cause counted more than once per frame: ' + cause)
        if present['rects'] < sum(present[k] for k in ('absent', 'explicit_full_rect', 'explicit_regions')):
            raise ValueError('nonempty Presents exceed clipped rectangle count')
        without_reduction = sum(render['damage_full_' + reason + '_count'] for reason in
                                ('no_table', 'disabled', 'unknown_age', 'no_history',
                                 'beyond_history', 'damage_unavailable'))
        executed = sum(present[k] for k in ('absent', 'explicit_full_rect', 'explicit_regions', 'effective_empty'))
        return {'status': 'VALID', 'counter_span_seconds': span,
                'counter_coverage': span / ((end - start) / 1e6),
                'frames': {'full': full, 'partial': partial},
                'full_without_usable_reduction': without_reduction,
                'full_plan_unspecified': render['damage_full_plan_unspecified_count'],
                'render': render,
                'causes': causes, 'present': present,
                'executed_presents': executed,
                'per_executed_present': ({
                    'full_output_frames': full / executed,
                    'partial_output_frames': partial / executed,
                    'repaint_pixels': render['composition_repaint_pixels_count'] / executed,
                    'declared_rect_pixels': present['rect_pixels'] / executed,
                    'causes': {k: v / executed for k, v in causes.items()},
                } if executed else None),
                'owner_continuity': 'Caller must select one uninterrupted Session/native-owner lifetime. '
                                    'Counter monotonicity alone does not prove that identity.',
                'scope': 'Overlapping causes at the rendered age; independent executed-Pixmap cohort. '
                         'rect_pixels is a clipped sum, not a union. No savings or damage-reduction claim.'}
    except (ValueError, KeyError, TypeError, IndexError) as error:
        return {'status': 'INVALID', 'error': str(error)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('log', type=Path)
    parser.add_argument('--start-usec', type=int, required=True)
    parser.add_argument('--end-usec', type=int, required=True)
    args = parser.parse_args()
    result = attribute(args.log.read_text(), args.start_usec, args.end_usec)
    print(json.dumps(result, indent=2, sort_keys=True))
    return 0 if result['status'] == 'VALID' else 2


if __name__ == '__main__':
    raise SystemExit(main())
