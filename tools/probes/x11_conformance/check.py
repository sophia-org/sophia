#!/usr/bin/env python3
"""Build and run the isolated X11 gate, without inherited live-session opt-ins."""
import argparse
import json
from pathlib import Path
import sys

from run import HERE, ROOT, bounded, clean_environment


def run_command(command, env):
    return bounded(command, 1800, cwd=ROOT, env=env)[0]


def profile_commands(profile, output, target):
    if profile == 'native-input':
        return [[sys.executable, '-B', str(HERE / 'native.py'), '--output', str(output),
                 '--target-dir', str(target)]]
    package, example = ('sophia-x-authority', 'x11_conformance_host') if profile == 'core' else (
        'sophia-session', 'native_input_conformance_host')
    return [['cargo', 'build', '--offline', '-p', package, '--example', example],
            [sys.executable, '-B', str(HERE / 'run.py'), '--host',
             str(target / 'debug/examples' / example), '--output', str(output),
             '--profile', profile]]


def read_profile(output, status):
    try:
        report = json.loads((output / 'report.json').read_text())
        if (status != 0 or report['status'] != 'PASS' or report['required'] <= 0
                or report['executed'] != report['required'] or report['failures']):
            return {'status': 'FAIL', 'detail': f'profile exit {status} or unmet obligations'}
        return {'status': 'PASS', 'required': report['required'], 'executed': report['executed']}
    except (OSError, ValueError, KeyError, TypeError):
        return {'status': 'NORESULT', 'detail': f'profile exit {status}; missing/invalid report'}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True, help='new evidence directory')
    parser.add_argument('--target-dir', type=Path, default=ROOT / '.artifacts/x11-conformance-target')
    parser.add_argument('--profile', choices=['core', 'native-input', 'xtest', 'all'], default='core')
    args = parser.parse_args()
    if args.output.exists():
        parser.error('--output must not exist')
    env = clean_environment()
    target = args.target_dir.resolve()
    env['CARGO_TARGET_DIR'] = str(target)
    for name in ('test_gate.py', 'test_native.py', 'test_xtest.py', 'test_profiles.py'):
        status = run_command([sys.executable, '-B', '-W', 'error', '-m', 'unittest', 'discover',
                              '-s', str(HERE), '-p', name], env)
        if status:
            return status
    if args.profile in ('xtest', 'all'):
        # Direct entry returns BLOCKED, not success, if any kernel test skipped.
        for name in ('test_isolation.py', 'test_xts_adapter.py'):
            status = run_command([sys.executable, '-B', str(HERE / name)], env)
            if status:
                return status
    selected = ['core', 'native-input', 'xtest'] if args.profile == 'all' else [args.profile]
    if args.profile == 'all':
        args.output.mkdir(parents=True, exist_ok=False)
    outcomes = {}
    for profile in selected:
        output = (args.output / profile if args.profile == 'all' else args.output).resolve()
        status = 0
        for command in profile_commands(profile, output, target):
            status = run_command(command, env)
            if status:
                break
        outcomes[profile] = read_profile(output, status)
        if args.profile != 'all':
            return 0 if outcomes[profile]['status'] == 'PASS' else status or 1
    report = {'status': 'PASS' if all(item['status'] == 'PASS' for item in outcomes.values()) else 'FAIL',
              'profiles': outcomes, 'xts5': {'status': 'NOT_RUN',
                                            'reason': 'separate dependency and selected-purpose adapter'}}
    (args.output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    return 0 if report['status'] == 'PASS' else 1


if __name__ == '__main__':
    raise SystemExit(main())
