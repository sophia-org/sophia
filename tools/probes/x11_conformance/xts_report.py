"""Strict selected-purpose TET journal accounting; never intersect away missing cases."""
import argparse
import json
from pathlib import Path

# TET's own results, then the three the suite adds in xts5/tet_code: WARNING
# (a pass with a caveat), FIP (further information pending) and ABORT.
VERDICTS = {0: 'PASS', 1: 'FAIL', 2: 'UNRESOLVED', 3: 'NOTINUSE',
            4: 'UNSUPPORTED', 5: 'UNTESTED', 6: 'UNINITIATED', 7: 'NORESULT',
            101: 'WARNING', 102: 'FIP', 103: 'ABORT'}


def parse_journal(text):
    activities, started, results = {}, set(), {}
    for line in text.splitlines():
        fields = line.split('|')
        if fields[0] not in ('10', '200', '220'):
            continue
        if len(fields) != 3:
            raise ValueError(f'malformed journal record: {line}')
        values = fields[1].split()
        if fields[0] == '10':
            activity, case = values[:2]
            if activity in activities:
                raise ValueError('duplicate activity identity')
            activities[activity] = case
            continue
        activity, purpose = values[:2]
        if activity not in activities:
            raise ValueError('purpose has no test-case start')
        key = (activities[activity], purpose)
        if fields[0] == '200':
            if key in started:
                raise ValueError('duplicate purpose start')
            started.add(key)
        else:
            if key not in started or key in results:
                raise ValueError('unstarted or duplicate purpose result')
            code, text = int(values[2]), fields[2].strip()
            status = VERDICTS.get(code)
            if status is None:
                # A result code this table does not know: the journal's own
                # name for it is the verdict, and the report carries it.
                status = text or f'RESULT-{code}'
            elif status != text:
                raise ValueError(f'numeric and textual verdict disagree: {line}')
            results[key] = status
    if not started:
        raise ValueError('empty journal: no purposes started')
    return started, results


DECLARABLE = {'FAIL', 'UNRESOLVED', 'NOTINUSE', 'UNSUPPORTED', 'UNTESTED', 'WARNING', 'FIP'}


def declared_expectation(row):
    """What a manifest row expects of its purpose: PASS unless it declares
    another disposition, with a reason. A declaration names a purpose the
    suite or the authority cannot pass today and says why; a row that
    declares without saying why is refused, and NORESULT and UNINITIATED
    cannot be declared at all, because they describe a run, not a purpose."""
    expected = row.get('expected', 'PASS')
    if expected == 'PASS':
        return 'PASS'
    if expected not in DECLARABLE:
        raise ValueError(f'{row["case"]} purpose {row["purpose"]}: undeclarable expectation {expected!r}')
    if not str(row.get('reason', '')).strip():
        raise ValueError(f'{row["case"]} purpose {row["purpose"]}: a declared {expected} needs a reason')
    return expected


def evaluate_journal(expected, journal, process_status=0):
    keys = [(row['case'], str(row['purpose'])) for row in expected]
    if not keys or len(keys) != len(set(keys)):
        raise ValueError('expected purposes must be nonempty and unique')
    expectations = {(row['case'], str(row['purpose'])): declared_expectation(row) for row in expected}
    required = set(keys)
    failures = []
    started, results = parse_journal(journal)
    passed, declared = 0, {}
    for key in sorted(required | started):
        if key not in required:
            failures.append(f'{key}: unmanifested purpose')
            continue
        if key not in started:
            failures.append(f'{key}: MISSING purpose')
            continue
        observed = results.get(key, 'NORESULT')
        wanted = expectations[key]
        if observed == wanted == 'PASS':
            passed += 1
        elif observed == wanted:
            # A declared disposition, met: the suite said what the manifest
            # said it would say. Counted so a report never hides how much of
            # a PASS is declaration.
            declared[wanted] = declared.get(wanted, 0) + 1
        elif observed == 'PASS':
            failures.append(f'{key}: PASS but declared {wanted}; the manifest is stale')
        elif wanted == 'PASS':
            failures.append(f'{key}: {observed}')
        else:
            failures.append(f'{key}: {observed} (declared {wanted})')
    # tcc exits non-zero when a test program did, and a test program does
    # when a purpose did not PASS. A run whose only non-PASS purposes were
    # declared, all started and all completed, has an exit the manifest
    # already explains; anything else about the exit -- a timeout, a purpose
    # nobody declared, a journal short of the manifest -- stands as it did.
    exit_explained = (process_status not in (0, 124) and not failures and bool(declared)
                      and started == required and len(results) == len(required))
    if process_status != 0 and not exit_explained:
        failures.insert(0, f'XTS process exit {process_status} (124 means TIMEOUT)')
    return {'status': 'FAIL' if failures else 'PASS', 'failures': failures,
            'required': len(required), 'started': len(started), 'completed': len(results),
            'passed': passed, 'declared': declared, 'process_status': process_status,
            'exit_explained_by_declarations': exit_explained}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--expected', type=Path, required=True)
    parser.add_argument('--journal', type=Path, required=True)
    parser.add_argument('--process-status', type=int, default=0)
    args = parser.parse_args()
    try:
        report = evaluate_journal(json.loads(args.expected.read_text()), args.journal.read_text(),
                                  args.process_status)
    except (ValueError, KeyError, IndexError, OSError) as error:
        report = {'status': 'FAIL', 'failures': [str(error)]}
    print(json.dumps(report, indent=2))
    return 0 if report['status'] == 'PASS' else 1


if __name__ == '__main__':
    raise SystemExit(main())
