#!/usr/bin/env python3
"""Optional XTS5 adapter, confined to a private display socket inside bubblewrap.

XTS is a separate dependency, not part of yserver. No /home/jos paths or
operator DISPLAY are used. Existing XTS results/configuration are not mutated.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time

from run import bounded, digest
from isolation import Mount, launch, validate_entry
from xts_report import evaluate_journal


def dependency_errors(root, bwrap):
    errors = []
    if not root or not (root / 'check.sh').is_file():
        errors.append(f'XTS checkout with check.sh missing: {root}; yserver is not XTS')
    if root and not (root / 'xts5').is_dir():
        errors.append(f'built XTS5 suite directory missing: {root / "xts5"}')
    if root and not any(p.is_file() and os.access(p, os.X_OK) for p in root.glob('**/tcc')) and not shutil.which('tcc'):
        errors.append('TET tcc executable missing from the XTS checkout and PATH')
    if not bwrap:
        errors.append('bubblewrap unavailable: private /tmp, network and device namespaces are required')
    return errors


WORK = Path('/work/run')


def inner_command():
    return ['/usr/bin/python3', '-B',
            str(WORK / 'harness/tools/probes/x11_conformance/xts.py'), '--inside',
            '--activation-fd', '{activation_fd}', '--host', str(WORK / 'host')]


def inside(host, activation_fd):
    validate_entry(activation_fd)
    # These values name only endpoints constructed after entry validation.
    os.environ.update(DISPLAY=':99', XAUTHORITY='/tmp/unused-Xauthority',
                      TET_ROOT=str(WORK / 'xts'))
    os.chdir(WORK / 'xts')
    configuration = json.loads((WORK / 'selection.json').read_text())
    Path('/tmp/.X11-unix').mkdir(mode=0o700)
    sock = Path('/tmp/.X11-unix/X99')
    with (WORK / 'host.log').open('wb') as log:
        server = subprocess.Popen([str(host), str(sock)], stdout=log, stderr=log)
        try:
            deadline = time.monotonic() + 5
            while not sock.exists():
                if server.poll() is not None or time.monotonic() > deadline:
                    raise RuntimeError('private XTS host did not bind')
                time.sleep(.01)
            # check.sh is supplied by the separate XTS checkout. Its status is
            # insufficient: the complete selected-purpose journal is checked.
            with (WORK / 'xts.log').open('wb') as output:
                status, _, _ = bounded(['bash', './check.sh', configuration['scenario']],
                                        configuration['timeout'], stdout=output, stderr=output)
            journals = list((WORK / 'xts/results').glob('*/journal'))
            if len(journals) != 1:
                raise RuntimeError(f'expected exactly one fresh journal, found {len(journals)}')
            shutil.copyfile(journals[0], WORK / 'journal')
            report = evaluate_journal(configuration['purposes'], journals[0].read_text(), status)
            (WORK / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
            return 0 if report['status'] == 'PASS' else 1
        finally:
            server.terminate()
            try:
                server.wait(timeout=2)
            except subprocess.TimeoutExpired:
                server.kill()
                server.wait()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--xts-root', type=Path)
    parser.add_argument('--host', type=Path, required=True)
    parser.add_argument('--expected', type=Path, help='explicit JSON array of mandatory {case,purpose} pairs')
    parser.add_argument('--scenario', help='selected scenario from the separate XTS checkout')
    parser.add_argument('--output', type=Path)
    parser.add_argument('--timeout', type=float, default=120)
    parser.add_argument('--inside', action='store_true', help=argparse.SUPPRESS)
    parser.add_argument('--activation-fd', type=int, default=-1, help=argparse.SUPPRESS)
    args = parser.parse_args()
    if args.inside:
        return inside(args.host, args.activation_fd)
    if not args.output:
        parser.error('--output is required and must be new')
    if not 0 < args.timeout <= 1800:
        parser.error('timeout must be in (0, 1800]')
    args.output.mkdir(parents=True, exist_ok=False)
    root = args.xts_root.resolve() if args.xts_root else None
    bwrap = shutil.which('bwrap')
    errors = dependency_errors(root, bwrap)
    if not args.expected or not args.expected.is_file():
        errors.append('mandatory purpose manifest missing: enumerate the selected built XTS test purposes')
    if not args.scenario or args.scenario.startswith('-'):
        errors.append('an explicit selected XTS scenario is required')
    if errors:
        report = {'status': 'BLOCKED', 'suite_executed': False, 'blockers': errors}
        (args.output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
        print(json.dumps(report, indent=2))
        return 2
    purposes = json.loads(args.expected.read_text())
    if not purposes:
        parser.error('empty purpose selection is not a suite')
    host = args.host.resolve(strict=True)
    with tempfile.TemporaryDirectory(prefix='sophia-xts-') as tmp:
        work = Path(tmp)
        # Private writable copy: no stale journal/config can supply a pass and
        # the external checkout is never patched. Keep any compiled binaries.
        shutil.copytree(root, work / 'xts', ignore=shutil.ignore_patterns('results', 'tetexec.cfg', '.git'))
        shutil.copytree(Path(__file__).resolve().parent,
                        work / 'harness/tools/probes/x11_conformance',
                        ignore=shutil.ignore_patterns('__pycache__'))
        shutil.copy2(host, work / 'host')
        (work / 'selection.json').write_text(json.dumps({'scenario': args.scenario,
                                                        'purposes': purposes, 'timeout': args.timeout}))
        result = launch(inner_command(), mounts=[Mount(work, str(WORK), writable=True)],
                        timeout=args.timeout + 15, bwrap=bwrap)
        status = result.returncode
        (args.output / 'adapter.log').write_bytes(result.stdout + result.stderr)
        for name in ('report.json', 'journal', 'host.log', 'xts.log', 'selection.json'):
            if (work / name).is_file():
                shutil.copyfile(work / name, args.output / name)
        report_path = args.output / 'report.json'
        if report_path.exists():
            report = json.loads(report_path.read_text())
        else:
            report = {'status': 'FAIL', 'failures': ['no XTS result: see adapter.log']}
        if status != 0:
            report['status'] = 'FAIL'
            report.setdefault('failures', []).append(f'adapter process exit {status}')
        report['host_sha256'] = digest(host)
        report['expected_sha256'] = digest(args.expected)
        report['xts_root'] = str(root)
        report['suite_executed'] = (args.output / 'journal').exists()
        report_path.write_text(json.dumps(report, indent=2) + '\n')
        print(json.dumps(report, indent=2))
        return 0 if report['status'] == 'PASS' else 1


if __name__ == '__main__':
    raise SystemExit(main())
