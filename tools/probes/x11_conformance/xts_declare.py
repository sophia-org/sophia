#!/usr/bin/env python3
"""Declare, in a purpose manifest, the dispositions a run observed -- each with
a reviewed reason.

A manifest lists what the suite has; a declaration says what the suite or the
authority cannot pass today and why. The dispositions are read from a real
journal, never typed, and every one needs a reason from a reviewed file keyed
by case or by `case#purpose`, or the declaration is refused: an observation
without a reason is a baseline, and a baseline accepts whatever it saw.
"""
import argparse
import json
from pathlib import Path
import sys

from xts_report import DECLARABLE, parse_journal


def declare(rows, journal, reasons):
    started, results = parse_journal(journal)
    out, missing = [], []
    for row in rows:
        key = (row['case'], str(row['purpose']))
        observed = results.get(key, 'NORESULT') if key in started else 'MISSING'
        row = {'case': row['case'], 'purpose': row['purpose']}
        if observed == 'PASS':
            out.append(row)
            continue
        if observed not in DECLARABLE:
            raise ValueError(f'{key}: {observed} describes the run, not the purpose; rerun rather than declare')
        reason = reasons.get(f'{key[0]}#{key[1]}') or reasons.get(key[0])
        if not reason:
            missing.append(f'{key[0]}#{key[1]}: {observed}')
            continue
        out.append({**row, 'expected': observed, 'reason': reason})
    if missing:
        raise ValueError('no reviewed reason for: ' + ', '.join(missing))
    return out


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--manifest', type=Path, required=True)
    parser.add_argument('--journal', type=Path, required=True)
    parser.add_argument('--reasons', type=Path, required=True,
                        help='JSON object: case path, or case#purpose, to the reviewed reason')
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    try:
        rows = declare(json.loads(args.manifest.read_text()), args.journal.read_text(),
                       json.loads(args.reasons.read_text()))
    except (ValueError, KeyError, OSError) as error:
        print(f'declaration failed: {error}', file=sys.stderr)
        return 1
    args.output.write_text(json.dumps(rows, indent=2) + '\n')
    declared = [row for row in rows if 'expected' in row]
    print(json.dumps({'purposes': len(rows), 'declared': len(declared)}, indent=2))
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
