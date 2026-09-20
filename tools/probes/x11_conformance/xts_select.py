#!/usr/bin/env python3
"""Enumerate a selected XTS5 scenario from a built checkout, never by hand.

Given case names, this reads the checkout's test sources for their purposes
(the `>>ASSERTION` markers of a TET `.m` file), writes the TET scenario file
the adapter's check.sh runs, and writes the exact purpose manifest the
adapter requires. A manifest typed by hand would be a guess about a suite
nobody ran; this one is read from the suite that will run.
"""
import argparse
import json
from pathlib import Path
import re
import sys

ASSERTION = re.compile(r'^>>ASSERTION\b', re.M)
CASE = re.compile(r'^[A-Za-z][A-Za-z0-9_]*$')


def case_sources(root, suite, case):
    """The `.m` sources of one case, or an error naming what is missing."""
    matches = sorted(p for p in (root / suite).glob(f'*/{case}/*.m') if p.is_file())
    if not matches:
        raise ValueError(f'case has no .m source under {root / suite}/*/{case}: {case}')
    return matches


def purposes_of(sources):
    count = sum(len(ASSERTION.findall(source.read_text(errors='replace'))) for source in sources)
    if count == 0:
        raise ValueError(f'no >>ASSERTION in {", ".join(str(s) for s in sources)}')
    return count


def scenario_path(root, suite, sources):
    """The path the suite's scenario file names the case by, `/Xlib3/XDestroyWindow`,
    which is also how TET's journal names it."""
    directory = sources[0].parent.relative_to(root / suite)
    return f'/{directory.as_posix()}'


def select(root, suite, cases):
    rows, lines = [], []
    seen = set()
    for case in cases:
        if not CASE.match(case):
            raise ValueError(f'invalid case name: {case}')
        if case in seen:
            raise ValueError(f'duplicate case: {case}')
        seen.add(case)
        sources = case_sources(root, suite, case)
        path = scenario_path(root, suite, sources)
        lines.append(path)
        rows.extend({'case': path, 'purpose': purpose} for purpose in range(1, purposes_of(sources) + 1))
    return lines, rows


def scenario_text(name, lines):
    body = '\n'.join(f'\t{line}' for line in lines)
    return f'{name}\n\t"selected scenario {name}: {len(lines)} cases"\n{body}\n'


def install(scenario_file, name, lines):
    """Append the scenario to the suite's own scenario file, replacing an
    earlier block of the same name; the runner selects scenarios by name from
    that one file."""
    text = scenario_file.read_text()
    block = scenario_text(name, lines)
    pattern = re.compile(rf'^{re.escape(name)}\n(?:[ \t].*\n?)*', re.M)
    if pattern.search(text):
        text = pattern.sub(lambda _: block, text, count=1)
    else:
        if not text.endswith('\n'):
            text += '\n'
        text += '\n' + block
    scenario_file.write_text(text)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--xts-root', type=Path, required=True)
    parser.add_argument('--suite', default='xts5')
    parser.add_argument('--scenario', required=True, help='scenario name, lowercase letters, digits, dashes')
    parser.add_argument('--case', action='append', default=[], help='a test case name such as XDestroyWindow')
    parser.add_argument('--manifest', type=Path, required=True, help='where to write the purpose manifest')
    parser.add_argument('--install', action='store_true',
                        help='also append the scenario to <root>/<suite>/tet_scen for check.sh')
    args = parser.parse_args()
    if not re.match(r'^[a-z0-9-]+$', args.scenario):
        parser.error('scenario must be lowercase letters, digits and dashes')
    if not args.case:
        parser.error('at least one --case is required')
    root = args.xts_root.resolve()
    try:
        lines, rows = select(root, args.suite, args.case)
    except (ValueError, OSError) as error:
        print(f'selection failed: {error}', file=sys.stderr)
        return 1
    args.manifest.write_text(json.dumps(rows, indent=2) + '\n')
    if args.install:
        scenario_file = root / args.suite / 'tet_scen'
        if not scenario_file.is_file():
            print(f'selection failed: no scenario file at {scenario_file}', file=sys.stderr)
            return 1
        install(scenario_file, args.scenario, lines)
    print(json.dumps({'scenario': args.scenario, 'cases': lines, 'purposes': len(rows)}, indent=2))
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
