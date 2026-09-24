#!/usr/bin/env python3
"""Launch only our own private socket host; there is deliberately no DISPLAY option."""
import argparse
import contextlib
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


# The private host's own artifacts, inside the writable results mount it
# shares with this runner. It refuses a path outside /work, which is one of
# the several things about its option set that are deliberate.
PRIVATE_ROOT = Path('/work/results/private')
# The instance the host stands up. Any values do, provided the cookie names
# the same instance the seat binding does.
PRIVATE_INSTANCE = 733
# Wide enough for the motion case, which clips to one less than each extent.
PRIVATE_SCREEN = (320, 240)


def private_host_command(host, activation, control, grants, lifetime_ms):
    """The exact option set `native_input_conformance_host` accepts.

    It names every option it takes and refuses everything else, including
    anything ambient: that refusal is the containment property M4 established,
    so the runner is what moves to meet it rather than the host being loosened
    to accept a positional socket.
    """
    width, height = PRIVATE_SCREEN
    return [str(host),
            '--activation-fd', str(activation),
            '--control-fd', str(control),
            '--socket', str(PRIVATE_ROOT / 'authority.sock'),
            '--cookie-file', str(PRIVATE_ROOT / 'cookie'),
            '--ready-file', str(PRIVATE_ROOT / 'ready'),
            '--instance', str(PRIVATE_INSTANCE),
            '--namespace', str(PRIVATE_INSTANCE),
            '--session-generation', '1',
            '--width', str(width), '--height', str(height),
            '--grants', grants,
            '--lifetime-ms', str(lifetime_ms)]


def private_host_case(host, manifest, case, order, timeout, log, private):
    """One XTEST case against the production private Session host.

    THE HOST AND THE CLIENT ARE SIBLINGS, NOT NESTED. A peer outside the
    host's PID namespace has no credentials it can read, so its setup dies at
    `x11_socket.rs` before any request: nesting the host cannot work, whatever
    else it would contain. Both therefore live in the namespace this runner
    was started in, and the host validates its own relayed activation rather
    than being vouched for by this entry.
    """
    specification = next(item for item in manifest['cases'] if item['id'] == case)
    _, implementations = profile_definition('xtest')
    PRIVATE_ROOT.mkdir(mode=0o700, parents=True, exist_ok=False)
    # ONE SECRET IN TWO PLACES THAT MUST AGREE. The host authenticates a setup
    # against this cookie and the client presents it by name; a client given a
    # different one would be refused for the right reason and prove nothing
    # about the request under test.
    cookie = secrets.token_bytes(32)
    (PRIVATE_ROOT / 'cookie').write_bytes(cookie)
    (PRIVATE_ROOT / 'cookie').chmod(0o600)
    authorization = PRIVATE_ROOT / 'authorization.json'
    authorization.write_text(json.dumps({'auth_name': 'SOPHIA-PRIVATE-INPUT-1',
                                         'auth_data_hex': cookie.hex()}))
    authorization.chmod(0o600)
    grants = 'verified' if specification.get('fixture', 'enabled') == 'enabled' else 'disabled'
    command = private_host_command(host, private['activation'], private['control'], grants,
                                   int(timeout * 1000) + 10_000)
    with log.open('wb') as output:
        server = subprocess.Popen(command, env=clean_environment(), stdout=output, stderr=output,
                                  close_fds=True, pass_fds=private['inherit'])
        stopped = False
        try:
            ready_deadline = time.monotonic() + 15
            while not (PRIVATE_ROOT / 'ready').exists():
                if server.poll() is not None:
                    return {'status': 'FAIL',
                            'detail': f'private host exited {server.returncode} before readiness'}
                if time.monotonic() >= ready_deadline:
                    return {'status': 'TIMEOUT', 'detail': 'private host readiness deadline'}
                time.sleep(0.01)
            result = execute_client(implementations, manifest, PRIVATE_ROOT / 'authority.sock',
                                    case, order, timeout, authorization, 'xtest')
            if server.poll() is not None:
                return {'status': 'FAIL',
                        'detail': f'private host exited unexpectedly {server.returncode}'}
            # The host reports its own collection on the way out, so its exit
            # is a statement about the service rather than about this case.
            with contextlib.suppress(OSError):
                os.write(private['stop'], b'stop\n')
            stopped = True
            try:
                returncode = server.wait(timeout=20)
            except subprocess.TimeoutExpired:
                server.kill()
                return {'status': 'FAIL', 'detail': 'private host ignored its stop command'}
            if returncode != 0:
                return {'status': 'FAIL', 'detail': f'private host collection failed {returncode}'}
            return result
        finally:
            if not stopped:
                with contextlib.suppress(OSError):
                    os.write(private['stop'], b'stop\n')
            if server.poll() is None:
                server.kill()
                server.wait()


def one_case(host, manifest, case, order, timeout, log, profile='core', private=None):
    if profile == 'xtest':
        return private_host_case(host, manifest, case, order, timeout, log, private)
    # Private directory has mode 0700, no global /tmp/.X11-unix listener.
    with tempfile.TemporaryDirectory(prefix='sophia-x11-') as tmp:
        sock = Path(tmp) / 'authority.sock'
        host_command = [str(host), str(sock)]
        authorization = None
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


def stop_descriptor(held, inherit):
    """The one capability this entry keeps: the control pipe's writing end.

    Named by subtraction rather than by argument, so a runner that was handed
    the reading end by mistake cannot end the host by writing to it.
    """
    remaining = sorted(set(held) - set(inherit))
    if len(remaining) != 1:
        raise SystemExit('private entry holds no single control capability')
    return remaining[0]


def case_budget(case, default):
    """A case's own absolute budget in seconds, when its manifest row declares
    one, else the runner's. A case that must wait out one of the authority's
    own allowances declares it here rather than stretching every case."""
    budget = float(case.get('timeout', default))
    if not 0 < budget <= 60:
        raise ValueError(f'{case["id"]}: case timeout {budget} is not within (0, 60] seconds')
    return budget


def isolated_case(host, case, order, timeout, log, profile='xtest'):
    from isolation import Mount, launch
    with tempfile.TemporaryDirectory(prefix='sophia-input-report-') as temporary:
        output = Path(temporary)
        command = ['/usr/bin/python3', '-B',
                   '/work/repo/tools/probes/x11_conformance/run.py',
                   '--inside', '--activation-fd', '{activation_fd}', '--profile', profile,
                   '--host', '/work/host', '--output', '/work/results',
                   '--case', case, '--order', order, '--timeout', str(timeout)]
        delegated, relayed, control = (), (), None
        if profile == 'xtest':
            # The host is a second entry beside the runner, so it needs its own
            # activation and its own control pipe. The runner keeps the writing
            # end: it is the only process in that namespace that knows when the
            # case is over, and the host's own ending is part of the evidence.
            control = os.pipe()
            command += ['--host-activation-fd', '{activation_fd_2}',
                        '--host-inherit', '{relayed_fds_2}',
                        '--control-fd', str(control[0])]
            delegated, relayed = (control[1],), ((control[0],),)
        try:
            result = launch(command, mounts=[Mount(HERE, '/work/repo/tools/probes/x11_conformance'),
                                            Mount(host, '/work/host'),
                                            Mount(output, '/work/results', writable=True)],
                            delegated_fds=delegated, relayed_activations=relayed,
                            timeout=timeout + 10)
        finally:
            for descriptor in control or ():
                with contextlib.suppress(OSError):
                    os.close(descriptor)
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
    parser.add_argument('--host-activation-fd', type=int, default=-1, help=argparse.SUPPRESS)
    parser.add_argument('--host-inherit', default='', help=argparse.SUPPRESS)
    parser.add_argument('--control-fd', type=int, default=-1, help=argparse.SUPPRESS)
    args = parser.parse_args()
    if not 0 < args.timeout <= 60:
        parser.error('timeout must be in (0, 60] seconds')
    if args.child:
        parser.error('clients require supervised private entry; direct socket paths are refused')
    # Validate the supervised entry before any fixture can create a socket.
    private = None
    if args.inside:
        from isolation import validate_entry
        held = validate_entry(args.activation_fd)
        if not all((args.host, args.output, args.case, args.order)):
            parser.error('private entry requires an exact fixture')
        if args.profile == 'xtest':
            # The host's entry is relayed, not vouched for: this checks only
            # that what it was handed is what this entry actually holds, and
            # the host validates the crossing itself.
            inherit = tuple(int(fd) for fd in args.host_inherit.split(',') if fd)
            if (args.host_activation_fd < 3 or args.control_fd < 3
                    or not {args.host_activation_fd, args.control_fd}.issubset(set(inherit))
                    or not set(inherit).issubset(held)):
                parser.error('the private host requires its own relayed activation')
            private = {'activation': args.host_activation_fd, 'control': args.control_fd,
                       'inherit': inherit, 'stop': stop_descriptor(held, inherit)}
    manifest, implementations = profile_definition(args.profile)
    if args.inside:
        result = one_case(args.host, manifest, args.case, args.order, args.timeout,
                          args.output / 'host.log', profile=args.profile, private=private)
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
                result = isolated_case(host, case['id'], order, case_budget(case, args.timeout),
                                       log, args.profile)
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
