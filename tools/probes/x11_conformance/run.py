#!/usr/bin/env python3
"""Launch only our own private socket host; there is deliberately no DISPLAY option."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import secrets
import time
import traceback

from cases import CASES
from report import evaluate, load_manifest
from inventory import check_inventory

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]


def clean_environment():
    return {k: v for k, v in os.environ.items()
            if not k.startswith(('SOPHIA_', 'HAGIA_', 'DBUS_', 'XDG_'))
            and k not in ('DISPLAY', 'XAUTHORITY', 'WAYLAND_DISPLAY', 'WAYLAND_SOCKET',
                          'PYTHONOPTIMIZE')}


def decode_result(status, stdout, stderr):
    if status == 124:
        return {'status': 'TIMEOUT', 'detail': 'client process deadline'}
    try:
        result = json.loads(stdout)
        assert result['status'] in ('PASS', 'FAIL', 'TIMEOUT')
    except (ValueError, KeyError, TypeError, AssertionError):
        return {'status': 'NORESULT', 'detail': f'exit={status}, stderr={stderr[:2000]!r}'}
    if status != 0 and result['status'] == 'PASS':
        return {'status': 'FAIL', 'detail': 'PASS with nonzero client exit'}
    return result


def bounded(command, timeout, **kwargs):
    """Kill only our new process group, including descendants, on an absolute timeout."""
    with subprocess.Popen(command, start_new_session=True, **kwargs) as child:
        try:
            stdout, stderr = child.communicate(timeout=timeout)
            return child.returncode, stdout, stderr
        except subprocess.TimeoutExpired:
            os.killpg(child.pid, signal.SIGKILL)
            stdout, stderr = child.communicate()
            return 124, stdout, stderr


def one_case(host, manifest, case, order, timeout, log, profile='core'):
    # Private directory has mode 0700, no global /tmp/.X11-unix listener.
    with tempfile.TemporaryDirectory(prefix='sophia-x11-') as tmp:
        sock = Path(tmp) / 'authority.sock'
        host_command = [str(host), str(sock)]
        authorization = None
        if profile == 'xtest':
            specification = next(item for item in manifest['cases'] if item['id'] == case)
            authorization = Path(tmp) / 'authorization.json'
            authorization.write_text(json.dumps({'auth_name': 'SOPHIA-PRIVATE-INPUT-1',
                                                 'auth_data_hex': secrets.token_hex(32)}))
            authorization.chmod(0o600)
            host_command += ['--private-input', specification.get('fixture', 'enabled'),
                             '--authorization-file', str(authorization)]
        with log.open('wb') as output:
            server = subprocess.Popen(host_command, env=clean_environment(),
                                      stdout=output, stderr=output, start_new_session=True)
            try:
                ready_deadline = time.monotonic() + 5
                while not sock.exists():
                    if server.poll() is not None:
                        return {'status': 'FAIL', 'detail': f'host exited {server.returncode} before bind'}
                    if time.monotonic() >= ready_deadline:
                        return {'status': 'TIMEOUT', 'detail': 'host bind deadline'}
                    time.sleep(0.01)
                # Already inside the per-case supervised namespace process.
                # Its external watchdog bounds even a nonsocket client hang.
                _, implementations = profile_definition(profile)
                result = execute_client(implementations, manifest, sock, case, order,
                                        timeout, authorization, profile)
                if server.poll() is not None:
                    return {'status': 'FAIL', 'detail': f'host exited unexpectedly {server.returncode}'}
                return result
            finally:
                if server.poll() is None:
                    os.killpg(server.pid, signal.SIGTERM)
                try:
                    server.wait(timeout=2)
                except subprocess.TimeoutExpired:
                    os.killpg(server.pid, signal.SIGKILL)
                    server.wait()


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def execute_client(implementations, manifest, sock, case, order, timeout,
                   authorization_path=None, profile='core'):
    try:
        context = {'socket': sock, 'order': '<' if order == 'little' else '>',
                   'deadline': time.monotonic() + timeout, 'case': case,
                   'extensions': manifest['extensions'],
                   'fixture_absence': manifest.get('fixture_absence', []),
                   'denied_extensions': manifest.get('intentional_absence', [])}
        if authorization_path:
            authorization = json.loads(authorization_path.read_text())
            context.update(auth_name=authorization['auth_name'].encode('ascii'),
                           auth_data=bytes.fromhex(authorization['auth_data_hex']))
        if profile == 'xtest':
            specification = next(item for item in manifest['cases'] if item['id'] == case)
            context['xtest_enabled'] = specification.get('fixture', 'enabled') == 'enabled'
            context['known_xtest_major'] = 146
        implementations[case](context)
        return {'status': 'PASS'}
    except TimeoutError as error:
        return {'status': 'TIMEOUT', 'detail': str(error)}
    except Exception as error:
        return {'status': 'FAIL', 'detail': f'{type(error).__name__}: {error}',
                'traceback': traceback.format_exc(limit=5)}


def profile_definition(profile):
    if profile == 'core':
        implementations = CASES
        manifest = load_manifest(HERE / 'manifest.json')
    else:
        from xtest_cases import CASES as implementations
        manifest = load_manifest(HERE / 'xtest_manifest.json')
    if set(implementations) != {case['id'] for case in manifest['cases']}:
        raise ValueError('implementation/manifest drift')
    return manifest, implementations


def isolated_case(host, case, order, timeout, log, profile='xtest'):
    from isolation import Mount, launch
    with tempfile.TemporaryDirectory(prefix='sophia-input-report-') as temporary:
        output = Path(temporary)
        command = ['/usr/bin/python3', '-B',
                   '/work/repo/tools/probes/x11_conformance/run.py',
                   '--inside', '--activation-fd', '{activation_fd}', '--profile', profile,
                   '--host', '/work/host', '--output', '/work/results',
                   '--case', case, '--order', order, '--timeout', str(timeout)]
        result = launch(command, mounts=[Mount(HERE, '/work/repo/tools/probes/x11_conformance'),
                                        Mount(host, '/work/host'),
                                        Mount(output, '/work/results', writable=True)],
                        timeout=timeout + 10)
        host_log = output / 'host.log'
        log.write_bytes(host_log.read_bytes() if host_log.exists() else result.stderr)
        log.with_suffix('.adapter.log').write_bytes(result.stderr)
        return decode_result(result.returncode, result.stdout, result.stderr)


def main():
    if not __debug__:
        raise SystemExit('conformance assertions require Python without -O')
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--host', type=Path)
    parser.add_argument('--output', type=Path)
    parser.add_argument('--timeout', type=float, default=3)
    parser.add_argument('--profile', choices=['core', 'xtest'], default='core')
    parser.add_argument('--child', type=Path, help=argparse.SUPPRESS)
    parser.add_argument('--case', help=argparse.SUPPRESS)
    parser.add_argument('--order', choices=['little', 'big'], help=argparse.SUPPRESS)
    parser.add_argument('--inside', action='store_true', help=argparse.SUPPRESS)
    parser.add_argument('--activation-fd', type=int, default=-1, help=argparse.SUPPRESS)
    args = parser.parse_args()
    if not 0 < args.timeout <= 60:
        parser.error('timeout must be in (0, 60] seconds')
    if args.child:
        parser.error('clients require supervised private entry; direct socket paths are refused')
    # Validate the supervised entry before any fixture can create a socket.
    if args.inside:
        from isolation import validate_entry
        validate_entry(args.activation_fd)
        if not all((args.host, args.output, args.case, args.order)):
            parser.error('private entry requires an exact fixture')
    manifest, implementations = profile_definition(args.profile)
    if args.inside:
        result = one_case(args.host, manifest, args.case, args.order, args.timeout,
                          args.output / 'host.log', profile=args.profile)
        print(json.dumps(result))
        return 0 if result['status'] == 'PASS' else 1
    if not args.host or not args.output:
        parser.error('--host and --output are required; --output must be new')
    host = args.host.resolve(strict=True)
    inventory = check_inventory(ROOT, manifest) if args.profile == 'core' else None
    args.output.mkdir(parents=True, exist_ok=False)
    results = []
    for case in manifest['cases']:
        for order in manifest['byte_orders']:
            try:
                log = args.output / f'{case["id"]}-{order}.host.log'
                result = isolated_case(host, case['id'], order, args.timeout, log, args.profile)
            except Exception as error:
                result = {'status': 'FAIL', 'detail': f'harness/host failure: {type(error).__name__}: {error}'}
            result.update(case=case['id'], byte_order=order)
            results.append(result)
            print(f'{case["id"]}/{order}: {result["status"]}', flush=True)
    report = evaluate(manifest, results)
    report['identity'] = {'host': str(host), 'host_sha256': digest(host),
                          'source_commit': subprocess.check_output(
                              ['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
                          'source_dirty': bool(subprocess.check_output(
                              ['git', 'status', '--porcelain'], cwd=ROOT, text=True)),
                          'harness_sha256': {p.name: digest(p) for p in HERE.iterdir()
                                             if p.is_file() and p.suffix in ('.py', '.json')}}
    report['scope'] = manifest['scope']
    report['request_inventory'] = inventory
    report['xts5'] = {'status': 'NOT_RUN', 'reason': 'separate explicit adapter invocation required'}
    (args.output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps({k: report[k] for k in ('status', 'required', 'executed', 'failures')}, indent=2))
    return 0 if report['status'] == 'PASS' else 1


if __name__ == '__main__':
    sys.exit(main())
