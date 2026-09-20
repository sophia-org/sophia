"""Containment regressions. Run directly: unavailable kernel isolation exits 2.

Unit discovery may represent an unavailable kernel with SKIP; callers requiring
containment MUST execute this module directly and require exit zero.
"""
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import isolation

HERE = Path(__file__).resolve().parent
ARGV = ['/usr/bin/python3', '-B', '/work/probes/isolation_probe.py', '{activation_fd}']


class ContractTests(unittest.TestCase):
    def test_no_ambient_root_or_environment(self):
        command = isolation.command('/bwrap', ARGV, [], 9)
        self.assertNotIn('--share-net', command)
        self.assertNotIn('--not-a-security-boundary', command)
        self.assertIn('--clearenv', command)
        self.assertIn('--new-session', command)
        self.assertIn('--unshare-all', command)
        self.assertIn('--cap-drop', command)
        mounts = [command[i + 1:i + 3] for i, item in enumerate(command) if item == '--ro-bind']
        self.assertNotIn(['/', '/'], mounts)
        self.assertEqual(mounts, [[p, p] for p in ('/usr/bin', '/usr/lib', '/usr/share')])

    def test_invalid_mounts_and_overlap(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for target in ('/', '/run', '/work', '/work/../run', 'relative'):
                with self.subTest(target=target), self.assertRaises(isolation.IsolationError):
                    isolation.command('/bwrap', ARGV, [isolation.Mount(root, target)], 3)
            with self.assertRaises(isolation.IsolationError):
                isolation.command('/bwrap', ARGV, [isolation.Mount(root, '/work/a'),
                                                  isolation.Mount(root, '/work/a/b')], 3)

    def test_socket_is_not_an_artifact(self):
        with tempfile.TemporaryDirectory() as temporary, socket.socket(socket.AF_UNIX) as sock:
            try:
                sock.bind(temporary + '/socket')
            except PermissionError as error:
                self.skipTest(f'BLOCKED: fixture socket bind denied: {error}')
            with self.assertRaises(isolation.IsolationError):
                isolation.command('/bwrap', ARGV, [isolation.Mount(Path(temporary), '/work/a')], 3)

    def test_direct_entry_fails_even_with_namespace_record(self):
        reader, writer = os.pipe()
        os.write(writer, json.dumps({'namespaces': isolation.namespace_ids(),
                                     'descriptors': {}}).encode())
        os.close(writer)
        with self.assertRaisesRegex(isolation.IsolationError, 'namespace'):
            isolation.validate_entry(reader)
        with self.assertRaises(isolation.IsolationError):
            isolation.validate_entry(reader)

    def test_invalid_activation_is_refused(self):
        with self.assertRaises(isolation.IsolationError):
            isolation.validate_entry(-1)

    def test_actual_namespace_descriptors_do_not_authorize_direct_entry(self):
        with isolation._activation((), ()) as (readers, namespace_fds, _carried, _relayed):
            result = subprocess.run(
                ['/usr/bin/python3', '-B', str(HERE / 'isolation_probe.py'), str(readers[0])],
                env=isolation.ENVIRONMENT, stdin=subprocess.DEVNULL,
                capture_output=True, pass_fds=(*readers, *namespace_fds), timeout=5)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(b'every required namespace', result.stderr)

    def test_no_fallback_api(self):
        # Failure is propagated, not offered to a fallback backend.
        with patch('isolation.shutil.which', return_value=None):
            with self.assertRaisesRegex(isolation.IsolationError, 'BLOCKED'):
                isolation.launch(ARGV)


class RealIsolationTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.mounts = [isolation.Mount(HERE, '/work/probes')]
        try:
            result = isolation.launch(ARGV, mounts=cls.mounts, timeout=10)
        except isolation.IsolationError as error:
            raise unittest.SkipTest(f'BLOCKED: {error}') from error
        if result.returncode:
            stderr = result.stderr.decode(errors='replace')
            if 'bwrap:' in stderr and any(reason in stderr for reason in (
                    'Operation not permitted', 'Permission denied', 'No permissions',
                    'Creating new namespace failed', 'not allowed')):
                raise unittest.SkipTest(f'BLOCKED: kernel bubblewrap unavailable: {stderr.strip()}')
            raise RuntimeError(f'private entry failed: {stderr}')

    def invoke(self, *extra, **kwargs):
        result = isolation.launch(ARGV + list(extra), mounts=self.mounts, timeout=10, **kwargs)
        self.assertEqual(result.returncode, 0, result.stderr.decode(errors='replace'))
        return json.loads(result.stdout)

    def test_activation_and_poisoned_environment(self):
        poison = {key: '/fabricated/no-endpoint' for key in (
            'DISPLAY', 'XAUTHORITY', 'WAYLAND_DISPLAY', 'WAYLAND_SOCKET',
            'DBUS_SESSION_BUS_ADDRESS', 'DBUS_SYSTEM_BUS_ADDRESS', 'XDG_RUNTIME_DIR',
            'SOPHIA_PRIVATE_INPUT', 'HAGIA_SOCKET', 'LD_PRELOAD', 'PYTHONPATH')}
        with patch.dict(os.environ, poison):
            report = self.invoke()
        self.assertEqual(report['environment'], isolation.ENVIRONMENT)
        self.assertTrue(report['validated'])

    def test_environment_filter_mutation_is_detected(self):
        original = isolation.command

        def poisoned(*args):
            command = original(*args)
            marker = command.index('--')
            command[marker:marker] = ['--setenv', 'DISPLAY', '/fabricated/socket']
            return command

        with patch('isolation.command', side_effect=poisoned):
            result = isolation.launch(ARGV, mounts=self.mounts, timeout=10)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(b'ambient environment', result.stderr)

    def test_network_namespace_mutation_is_detected(self):
        original = isolation.command

        def shared_network(*args):
            command = original(*args)
            command.insert(command.index('--unshare-all') + 1, '--share-net')
            return command

        with patch('isolation.command', side_effect=shared_network):
            result = isolation.launch(ARGV, mounts=self.mounts, timeout=10)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(b'every required namespace', result.stderr)

    def test_fabricated_outside_socket_and_mount_mutation(self):
        # Deliberately outside /tmp: a tmpfs /tmp alone must not satisfy this.
        artifacts = Path('/home/niltempus/dev/sophia-stack/.artifacts')
        artifacts.mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(prefix='isolation-', dir=artifacts) as temporary:
            path = str(Path(temporary) / 'endpoint')
            with socket.socket(socket.AF_UNIX) as server:
                server.bind(path)
                server.listen(4)
                server.settimeout(1)
                with socket.socket(socket.AF_UNIX) as control:
                    control.connect(path)
                    accepted, _ = server.accept()
                    accepted.close()
                self.assertFalse(self.invoke('--socket', path)['reachable'])
                original = isolation.command

                def exposed(*args):
                    command = original(*args)
                    marker = command.index('--')
                    command[marker:marker] = ['--ro-bind', temporary, temporary]
                    return command

                with patch('isolation.command', side_effect=exposed):
                    mutation = self.invoke('--socket', path)
                with self.assertRaises(AssertionError):
                    self.assertFalse(mutation['reachable'])
                accepted, _ = server.accept()
                accepted.close()

    def test_explicit_descriptor_delegation(self):
        reader, writer = socket.socketpair()
        try:
            reader.settimeout(1)
            report = self.invoke('--delegated', str(writer.fileno()),
                                 delegated_fds=(writer.fileno(),))
            self.assertTrue(report['delegated'])
            self.assertEqual(reader.recv(100), b'authorized instance capability')
        finally:
            reader.close()
            writer.close()

    def test_relayed_activation_starts_a_second_validated_entry(self):
        # The entry that crosses is not always the one that needs the socket.
        # A relayed activation lets the first entry start a second beside it,
        # in the same namespaces, which validates its own crossing and is told
        # only about its own capability.
        reader, writer = socket.socketpair()
        try:
            reader.settimeout(1)
            relayed = writer.fileno()
            code = ('import json, os, subprocess, sys\n'
                    "sys.path.insert(0, '/work/iso')\n"
                    'from isolation import validate_entry\n'
                    'held = validate_entry(int(sys.argv[1]))\n'
                    'second, capability = int(sys.argv[2]), int(sys.argv[4])\n'
                    "inherited = tuple(int(fd) for fd in sys.argv[3].split(','))\n"
                    'assert {second, capability} <= held, sorted(held)\n'
                    'assert set(inherited) <= held, (sorted(inherited), sorted(held))\n'
                    "run = subprocess.run(['/usr/bin/python3', '-B',\n"
                    "    '/work/iso/isolation_probe.py', str(second),\n"
                    "    '--delegated', str(capability)], env=dict(os.environ),\n"
                    '    stdin=subprocess.DEVNULL, capture_output=True,\n'
                    '    pass_fds=inherited, timeout=10)\n'
                    "print(json.dumps({'second': run.returncode,\n"
                    "    'reason': run.stderr.decode()[-200:]}))\n")
            result = isolation.launch(
                ['/usr/bin/python3', '-B', '-c', code, '{activation_fd}',
                 '{activation_fd_2}', '{relayed_fds_2}', str(relayed)],
                mounts=[isolation.Mount(HERE, '/work/iso')],
                relayed_activations=((relayed,),), timeout=30)
            self.assertEqual(result.returncode, 0, result.stderr[-400:])
            report = json.loads(result.stdout)
            self.assertEqual(report['second'], 0, report['reason'])
            self.assertEqual(reader.recv(100), b'authorized instance capability')
        finally:
            reader.close()
            writer.close()

    def test_inherited_connected_descriptor_closed_and_mutation_detected(self):
        reader, writer = socket.socketpair()
        try:
            writer.set_inheritable(True)
            self.invoke()  # Entry checks every inherited descriptor.
            # Independently prove close_fds itself prevents using the connected
            # capability, rather than relying only on the entry validator.
            code = ('import os\ntry:\n os.write(' + str(writer.fileno())
                    + ',b"leaked")\nexcept OSError:\n print("CLOSED")\n'
                      'else:\n print("REACHABLE")')
            raw = ['/usr/bin/python3', '-c', code]
            result = isolation.launch(raw, timeout=10)
            self.assertEqual(result.stdout.strip(), b'CLOSED')
            popen = subprocess.Popen

            def leaked(*args, **kwargs):
                kwargs['pass_fds'] = (*kwargs['pass_fds'], writer.fileno())
                return popen(*args, **kwargs)

            with patch('isolation.subprocess.Popen', side_effect=leaked):
                result = isolation.launch(ARGV, mounts=self.mounts, timeout=10)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn(b'unlisted inherited descriptor', result.stderr)
            with patch('isolation.subprocess.Popen', side_effect=leaked):
                raw_mutant = isolation.launch(raw, timeout=10)
            with self.assertRaises(AssertionError):
                self.assertEqual(raw_mutant.stdout.strip(), b'CLOSED')
            reader.settimeout(1)
            self.assertEqual(reader.recv(100), b'leaked')
        finally:
            reader.close()
            writer.close()

    def test_absolute_timeout(self):
        result = isolation.launch(['/usr/bin/python3', '-c', 'import time;time.sleep(60)'],
                                  timeout=.2)
        self.assertEqual(result.returncode, 124)


if __name__ == '__main__':
    suite = unittest.defaultTestLoader.loadTestsFromModule(__import__(__name__))
    result = unittest.TextTestRunner(verbosity=2).run(suite)
    report = {'status': 'FAIL' if not result.wasSuccessful() else 'BLOCKED' if result.skipped else 'PASS',
              'tests_run': result.testsRun, 'blockers': [reason for _, reason in result.skipped]}
    print(json.dumps(report))
    raise SystemExit(1 if not result.wasSuccessful() else 2 if result.skipped else 0)
