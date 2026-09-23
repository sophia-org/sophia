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
TEST_TYPE = re.compile(r'^[A-Z][A-Z0-9_]*$')


def case_sources(root, suite, case):
    """The `.m` source of one case: `xts5/<section>/<case>.m`, or, for a case
    that carries data files, `xts5/<section>/<case>/<case>.m`."""
    matches = sorted(p for pattern in (f'*/{case}.m', f'*/{case}/{case}.m')
                     for p in (root / suite).glob(pattern) if p.is_file())
    if not matches:
        raise ValueError(f'case has no .m source under {root / suite}/*/{case}.m or */{case}/{case}.m: {case}')
    return matches


def purposes_of(sources):
    count = sum(len(ASSERTION.findall(source.read_text(errors='replace'))) for source in sources)
    if count == 0:
        raise ValueError(f'no >>ASSERTION in {", ".join(str(s) for s in sources)}')
    return count


def excluded_purposes(sources, test_types):
    """The one-based purposes whose code sets `test_type = <T>;` for an
    excluded T. A purpose is the text from its `>>ASSERTION` to the next, so
    the assignment is read from the purpose that makes it, not the file.
    Named exclusions are the only way a purpose leaves a selection: the
    manifest then says which and why, rather than a case being dropped whole.
    Purposes are numbered across a case's sources in order, as TET does."""
    if not test_types:
        return set()
    excluded, offset = set(), 0
    for source in sources:
        text = source.read_text(errors='replace')
        starts = [match.start() for match in ASSERTION.finditer(text)]
        for index, start in enumerate(starts):
            end = starts[index + 1] if index + 1 < len(starts) else len(text)
            block = text[start:end]
            if any(re.search(rf'^\s*test_type\s*=\s*{re.escape(t)}\s*;', block, re.M) for t in test_types):
                excluded.add(offset + index + 1)
        offset += len(starts)
    return excluded


def ic_list(kept):
    """TET's invocable-component list for a scenario line: `{1,2,4-6}`."""
    ranges, start, previous = [], None, None
    for purpose in sorted(kept):
        if start is None:
            start = previous = purpose
        elif purpose == previous + 1:
            previous = purpose
        else:
            ranges.append((start, previous))
            start = previous = purpose
    if start is not None:
        ranges.append((start, previous))
    return '{' + ','.join(f'{a}' if a == b else f'{a}-{b}' for a, b in ranges) + '}'


def scenario_path(root, suite, sources):
    """The path the suite's scenario file names the case by, which is the
    source's path under the suite without its suffix: `/Xlib4/XDestroyWindow`
    for a file case, `/Xlib4/XMapWindow/XMapWindow` for a directory case. TET's
    journal names the case the same way."""
    return '/' + sources[0].relative_to(root / suite).with_suffix('').as_posix()


def select(root, suite, cases, exclude_test_types=()):
    rows, lines, exclusions = [], [], []
    seen = set()
    for case in cases:
        if not CASE.match(case):
            raise ValueError(f'invalid case name: {case}')
        if case in seen:
            raise ValueError(f'duplicate case: {case}')
        seen.add(case)
        sources = case_sources(root, suite, case)
        path = scenario_path(root, suite, sources)
        total = purposes_of(sources)
        excluded = excluded_purposes(sources, exclude_test_types)
        kept = [purpose for purpose in range(1, total + 1) if purpose not in excluded]
        if not kept:
            raise ValueError(f'every purpose of {path} is excluded; drop the case instead')
        lines.append(path if not excluded else path + ic_list(kept))
        rows.extend({'case': path, 'purpose': purpose} for purpose in kept)
        exclusions.extend({'case': path, 'purpose': purpose, 'test_types': list(exclude_test_types)}
                          for purpose in sorted(excluded))
    return lines, rows, exclusions


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
    parser.add_argument('--exclude-test-type', action='append', default=[],
                        help='leave out every purpose whose code sets test_type to this name, '
                             'such as TOO_LONG; the exclusions are written beside the manifest')
    args = parser.parse_args()
    if not re.match(r'^[a-z0-9-]+$', args.scenario):
        parser.error('scenario must be lowercase letters, digits and dashes')
    if not args.case:
        parser.error('at least one --case is required')
    if any(not TEST_TYPE.match(name) for name in args.exclude_test_type):
        parser.error('a test type is an upper-case identifier such as TOO_LONG')
    root = args.xts_root.resolve()
    try:
        lines, rows, exclusions = select(root, args.suite, args.case, tuple(args.exclude_test_type))
    except (ValueError, OSError) as error:
        print(f'selection failed: {error}', file=sys.stderr)
        return 1
    args.manifest.write_text(json.dumps(rows, indent=2) + '\n')
    if args.exclude_test_type:
        args.manifest.with_name(args.manifest.stem + '.excluded.json').write_text(
            json.dumps(exclusions, indent=2) + '\n')
    if args.install:
        scenario_file = root / args.suite / 'tet_scen'
        if not scenario_file.is_file():
            print(f'selection failed: no scenario file at {scenario_file}', file=sys.stderr)
            return 1
        install(scenario_file, args.scenario, lines)
    print(json.dumps({'scenario': args.scenario, 'cases': lines, 'purposes': len(rows),
                      'excluded': len(exclusions)}, indent=2))
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
