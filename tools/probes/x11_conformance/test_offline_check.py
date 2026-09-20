"""Offline wrapper regressions; these do not execute cargo xtask check."""
from pathlib import Path
import subprocess
import json
import os
import sys
import tempfile
import unittest
from unittest.mock import patch

import offline_check as gate


class OfflineCheckTests(unittest.TestCase):
    def repository(self, base):
        source = base / 'original'
        source.mkdir()
        gate.git(source, '-c', 'init.templateDir=', 'init', '--quiet')
        (source / 'tracked.txt').write_text('the committed fixture\n')
        gate.git(source, 'add', 'tracked.txt')
        gate.git(source, '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid',
                 '-c', 'commit.gpgSign=false', 'commit', '--quiet', '-m', 'fixture')
        gate.git(source, '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid',
                 '-c', 'commit.gpgSign=false', 'commit', '--quiet', '--allow-empty', '-m', 'child fixture')
        return source

    def test_snapshot_preserves_exact_commit_without_worktree_links(self):
        with tempfile.TemporaryDirectory() as temporary:
            base = Path(temporary)
            source = self.repository(base)
            destination = base / 'copy'
            report = gate.snapshot(source, destination)
            self.assertEqual(report['commit'], gate.git(source, 'rev-parse', 'HEAD'))
            self.assertEqual(report['tree'], gate.git(destination, 'rev-parse', 'HEAD^{tree}'))
            self.assertEqual(gate.git(destination, 'rev-parse', 'HEAD^'),
                             gate.git(source, 'rev-parse', 'HEAD^'))
            self.assertFalse(report['dirty'])
            self.assertEqual(len(report['archive_sha256']), 64)
            self.assertFalse((destination / '.git/commondir').exists())
            self.assertFalse((destination / '.git/objects/info/alternates').exists())
            self.assertEqual(gate.git(destination, 'remote'), '')
            (source / 'tracked.txt').write_text('modified host checkout\n')
            self.assertEqual((destination / 'tracked.txt').read_text(), 'the committed fixture\n')
            self.assertFalse(gate.source_state(destination)['dirty'])

    def test_full_sibling_arguments_require_both_explicit_pairs(self):
        with tempfile.TemporaryDirectory() as temporary:
            source = Path(temporary)
            valid = (source, '1' * 40)
            for values in ({}, {'hagia': valid}, {'narthex': valid},
                           {'hagia': (source, None), 'narthex': valid},
                           {'hagia': valid, 'narthex': (None, '2' * 40)}):
                with self.subTest(values=values), self.assertRaises(ValueError):
                    gate.sibling_arguments(False, values)
            selected = gate.sibling_arguments(False, {'hagia': valid, 'narthex': valid})
            self.assertEqual(set(selected), {'hagia', 'narthex'})
            self.assertEqual(gate.sibling_arguments(True, {}), {'hagia': None, 'narthex': None})
            for revision in ('HEAD', 'main', '1' * 8, 'A' * 40, '1' * 40 + '^', 'z' * 40):
                with self.subTest(revision=revision), self.assertRaisesRegex(ValueError, '40-hex'):
                    gate.sibling_arguments(True, {'hagia': (source, revision)})
            with self.assertRaises(ValueError):
                gate.sibling_arguments(True, {'hagia': (source, None)})

    def test_missing_sibling_args_block_main_before_tools_or_canonical_launch(self):
        with tempfile.TemporaryDirectory() as temporary:
            base = Path(temporary); source = self.repository(base)
            output, target = base / 'output', base / 'target'
            arguments = ['offline_check.py', '--source', str(source), '--output', str(output),
                         '--target-dir', str(target)]
            with patch.object(sys, 'argv', arguments), \
                    patch.object(gate, 'check_paths', return_value=(output, target)), \
                    patch.object(gate, 'toolchain_path') as toolchain, \
                    patch.object(gate, 'launch') as launch, patch('builtins.print'):
                self.assertEqual(gate.main(), 2)
            toolchain.assert_not_called(); launch.assert_not_called()
            report = json.loads((output / 'report.json').read_text())
            self.assertEqual(report['status'], 'BLOCKED')
            self.assertFalse(report['full_check_executed'])

    def test_identity_repository_keeps_only_pinned_raw_commit_with_no_external_git_links(self):
        with tempfile.TemporaryDirectory() as temporary:
            base = Path(temporary); source = self.repository(base)
            commit = gate.git(source, 'rev-parse', 'HEAD^')
            raw, tree = gate.commit_payload(source, commit)
            # Neither a moving HEAD nor unrelated working edits are inputs.
            (source / 'tracked.txt').write_text('unrelated host edit\n')
            (source / 'untracked.txt').write_text('host-only content\n')
            gate.git(source, 'config', 'remote.operator.url', '/must-not-be-copied')
            destination = base / 'identity'
            report = gate.identity_repository(source, commit, destination)
            self.assertEqual(gate.git(destination, 'rev-parse', 'HEAD'), commit)
            self.assertEqual(gate.git(destination, 'rev-list', '--count', 'HEAD'), '1')
            self.assertEqual(gate.commit_payload(destination, commit), (raw, tree))
            self.assertEqual(gate.git(destination, 'remote'), '')
            self.assertEqual(gate.git(destination, 'rev-parse', '--is-bare-repository'), 'false')
            self.assertEqual(gate.git(destination, 'cat-file', '--batch-all-objects',
                                      '--batch-check=%(objectname) %(objecttype)'), commit + ' commit')
            self.assertEqual({p.name for p in destination.iterdir()}, {'.git'})
            self.assertFalse(report['tree_objects_included']); self.assertFalse(report['checkout_files'])
            with self.assertRaises(subprocess.CalledProcessError):
                gate.git(destination, 'cat-file', '-e', tree)
            (destination / '.git/objects/info/alternates').write_text('/fabricated/external/objects\n')
            with self.assertRaisesRegex(ValueError, 'unexpected Git'):
                gate.identity_repository_state(destination, commit)

    def test_identity_copy_rejects_wrong_type_missing_object_and_changed_write_identity(self):
        with tempfile.TemporaryDirectory() as temporary:
            base = Path(temporary); source = self.repository(base)
            for value in ('0' * 40, gate.git(source, 'rev-parse', 'HEAD^{tree}')):
                with self.subTest(value=value), self.assertRaises((ValueError, subprocess.CalledProcessError)):
                    gate.identity_repository(source, value, base / value)
            commit = gate.git(source, 'rev-parse', 'HEAD')
            original = gate.subprocess.check_output
            def wrong_identity(command, **kwargs):
                if 'hash-object' in command:
                    return b'0' * 40 + b'\n'
                return original(command, **kwargs)
            with patch.object(gate.subprocess, 'check_output', side_effect=wrong_identity):
                with self.assertRaisesRegex(ValueError, 'copied commit object'):
                    gate.identity_repository(source, commit, base / 'bad-write')

    def test_identity_repository_rejects_external_git_directory_and_config(self):
        with tempfile.TemporaryDirectory() as temporary:
            base = Path(temporary); source = self.repository(base)
            commit = gate.git(source, 'rev-parse', 'HEAD')
            destination = base / 'identity'
            gate.identity_repository(source, commit, destination)
            config = destination / '.git/config'
            config.write_bytes(gate.IDENTITY_CONFIG + b'[include]\npath = /fabricated/host-config\n')
            with self.assertRaisesRegex(ValueError, 'unexpected Git'):
                gate.identity_repository_state(destination, commit)
            config.write_bytes(gate.IDENTITY_CONFIG)
            (destination / '.git').rename(base / 'external-git')
            (destination / '.git').symlink_to(base / 'external-git', target_is_directory=True)
            with self.assertRaisesRegex(ValueError, 'external Git'):
                gate.identity_repository_state(destination, commit)

    def test_sibling_environment_and_signatures_use_only_private_identity_repositories(self):
        with tempfile.TemporaryDirectory() as temporary:
            base = Path(temporary); source = self.repository(base)
            commit = gate.git(source, 'rev-parse', 'HEAD')
            directory = base / 'identities'
            gate.prepare_siblings({name: (source, commit) for name in gate.SIBLINGS}, directory)
            environment = {**gate.GIT_ENVIRONMENT, 'GNUPGHOME': str(base / 'private-gpg')}
            real_command = gate.verification_command
            def signed(command, environment, **kwargs):
                if 'verify-commit' in command:
                    return b''
                return real_command(command, environment, **kwargs)
            with patch.object(gate, 'verification_command', side_effect=signed) as verify:
                private, result = gate.verify_siblings(False, directory, environment, {'status': 'PASS'})
            signature_calls = [call for call in verify.call_args_list if 'verify-commit' in call.args[0]]
            self.assertEqual(len(signature_calls), 2)
            for name, call in zip(gate.SIBLINGS, signature_calls):
                self.assertEqual(call.args, (['/usr/bin/git', '-C', str(directory / name),
                                             'verify-commit', commit], environment))
                self.assertEqual(private[f'SOPHIA_{name.upper()}_ROOT'], str(directory / name))
                self.assertEqual(result[name]['signature'], 'PASS')
            self.assertNotIn('SOPHIA_HAGIA_ROOT', environment)
            gate.unchanged_siblings(result, directory)
            (directory / 'hagia/.git/HEAD').write_text('0' * 40 + '\n')
            with self.assertRaisesRegex(ValueError, 'HEAD'):
                gate.unchanged_siblings(result, directory)

    def test_unsigned_sibling_blocks_real_verification_without_ambient_keyring(self):
        with tempfile.TemporaryDirectory() as temporary:
            base = Path(temporary); source = self.repository(base)
            commit = gate.git(source, 'rev-parse', 'HEAD')
            directory = base / 'identities'
            gate.prepare_siblings({name: (source, commit) for name in gate.SIBLINGS}, directory)
            home = base / 'private-gpg'; home.mkdir(mode=0o700)
            environment = {**gate.GIT_ENVIRONMENT, 'GNUPGHOME': str(home)}
            with self.assertRaisesRegex(gate.VerificationError, 'hagia commit signature failed'):
                gate.verify_siblings(False, directory, environment, {'status': 'PASS'})

    def test_metadata_omitted_siblings_stay_not_run_and_full_missing_is_blocked(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary) / 'identities'
            gate.prepare_siblings({name: None for name in gate.SIBLINGS}, directory)
            with patch.object(gate, 'verification_command') as command:
                environment, result = gate.verify_siblings(True, directory, gate.ENVIRONMENT, {'status': 'NOT_RUN'})
                self.assertTrue(all(value['status'] == 'NOT_RUN' for value in result.values()))
                self.assertNotIn('SOPHIA_HAGIA_ROOT', environment)
                self.assertNotIn('SOPHIA_NARTHEX_ROOT', environment)
                with self.assertRaisesRegex(gate.VerificationError, 'explicit hagia'):
                    gate.verify_siblings(False, directory, gate.ENVIRONMENT, {'status': 'PASS'})
            command.assert_not_called()

    def test_dirty_tracked_and_untracked_source_refused_before_copy(self):
        with tempfile.TemporaryDirectory() as temporary:
            base = Path(temporary)
            source = self.repository(base)
            for name in ('tracked.txt', 'untracked.txt'):
                with self.subTest(name=name):
                    (source / name).write_text('not committed\n')
                    destination = base / ('copy-' + name)
                    with self.assertRaisesRegex(ValueError, 'dirty'):
                        gate.snapshot(source, destination)
                    self.assertFalse(destination.exists())
                    if name == 'tracked.txt':
                        gate.git(source, 'checkout', '--', name)
                    else:
                        (source / name).unlink()

    def test_changed_head_during_snapshot_is_not_relabelled(self):
        with tempfile.TemporaryDirectory() as temporary:
            base = Path(temporary)
            source = self.repository(base)
            real_state = gate.source_state
            reads = 0
            def changed(path):
                nonlocal reads
                result = real_state(path)
                if path == source:
                    reads += 1
                    if reads > 1:
                        result['commit'] = '0' * 40
                return result
            with patch.object(gate, 'source_state', side_effect=changed):
                with self.assertRaisesRegex(ValueError, 'changed while'):
                    gate.snapshot(source, base / 'copy')

    def test_direct_inside_refused_before_tool_or_device_inspection(self):
        with patch.object(gate, 'validate_entry', side_effect=gate.IsolationError('no activation')):
            with patch.object(gate.subprocess, 'check_output') as command:
                with self.assertRaisesRegex(gate.IsolationError, 'no activation'):
                    gate.inside(-1, True)
                command.assert_not_called()
        result = subprocess.run([sys.executable, '-B', str(Path(gate.__file__)),
                                 '--inside', '--validate-only'],
                                capture_output=True, timeout=5)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(b'activation must be a runner pipe', result.stderr)
        self.assertNotIn(b'cargo metadata', result.stderr)

    def test_inside_requires_no_render_nodes_even_after_activation(self):
        with patch.object(gate, 'validate_entry'), patch.object(gate.Path, 'exists', return_value=True):
            with self.assertRaisesRegex(gate.IsolationError, 'render-device'):
                gate.inside(7, True)

    def test_snapshot_git_cannot_use_ambient_configuration(self):
        self.assertEqual(gate.GIT_ENVIRONMENT['GIT_CONFIG_GLOBAL'], '/dev/null')
        self.assertEqual(gate.GIT_ENVIRONMENT['GIT_CONFIG_NOSYSTEM'], '1')
        self.assertEqual(gate.GIT_ENVIRONMENT['GIT_TERMINAL_PROMPT'], '0')
        for name in ('DISPLAY', 'WAYLAND_DISPLAY', 'DBUS_SESSION_BUS_ADDRESS',
                     'SOPHIA_RUN_REAL_GBM_SMOKE', 'HAGIA_SOCKET', 'SSH_AUTH_SOCK'):
            self.assertNotIn(name, gate.GIT_ENVIRONMENT)

    def test_temporary_filesystem_targets_refused(self):
        with patch.object(gate, 'git', return_value='/tmp/fabricated/.git'):
            with self.assertRaisesRegex(ValueError, 'disk-backed'):
                gate.check_paths(Path('/tmp/fabricated'), Path('/tmp/fabricated/.artifacts/output'),
                                 Path('/tmp/fabricated/.artifacts/target'))

    def test_hosts_fixture_is_private_regular_data_without_host_copy(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            gate.private_hosts(root)
            hosts = root / 'etc/hosts'
            self.assertTrue(hosts.is_file())
            self.assertFalse(hosts.is_symlink())
            self.assertEqual(hosts.read_text(), '127.0.0.1 localhost\n::1 localhost\n')
            self.assertEqual(list((root / 'etc').iterdir()), [hosts])

    def test_profile_target_link_names_only_the_owned_disk_target(self):
        with tempfile.TemporaryDirectory() as temporary:
            source = Path(temporary)
            gate.private_target_link(source)
            target = source / 'target'
            self.assertTrue(target.is_symlink())
            self.assertEqual(target.readlink(), Path('/work/target'))
            with self.assertRaises(FileExistsError):
                gate.private_target_link(source)

    def test_loader_cache_uses_private_libraries_and_no_host_configuration(self):
        with patch.object(gate.subprocess, 'run') as command:
            gate.private_loader_cache()
        command.assert_called_once_with(
            ['/usr/bin/ldconfig', '-X', '-i', '-f', '/dev/null',
             '-C', '/etc/ld.so.cache', '/usr/lib'], env=gate.ENVIRONMENT,
            stdin=subprocess.DEVNULL, capture_output=True, text=True,
            check=True, timeout=30)

    @staticmethod
    def elf(path, interpreter=None):
        """A minimal ELF64 executable: static when no interpreter is given,
        else carrying one PT_INTERP naming it. Headers only; never run."""
        phnum = 0 if interpreter is None else 1
        header = (b'\x7fELF' + bytes([2, 1, 1, 0]) + bytes(8)
                  + (2).to_bytes(2, 'little') + (62).to_bytes(2, 'little') + (1).to_bytes(4, 'little')
                  + bytes(8) + (64).to_bytes(8, 'little') + bytes(8) + bytes(4)
                  + (64).to_bytes(2, 'little') + (56).to_bytes(2, 'little') + phnum.to_bytes(2, 'little')
                  + bytes(6))
        body = header
        if interpreter is not None:
            name = interpreter.encode() + b'\0'
            program = ((3).to_bytes(4, 'little') + bytes(4) + (120).to_bytes(8, 'little')
                       + bytes(16) + len(name).to_bytes(8, 'little') + len(name).to_bytes(8, 'little')
                       + bytes(8))
            body = header + program + name
        path.write_bytes(body)
        path.chmod(0o755)

    def test_missing_required_search_tool_is_a_blocker_not_retired_source_debt(self):
        with tempfile.TemporaryDirectory() as temporary:
            with patch.dict(os.environ, {'PATH': temporary}):
                with self.assertRaisesRegex(ValueError, 'missing canonical check dependency: rg'):
                    gate.required_tool('rg')
            executable = Path(temporary) / 'rg'
            self.elf(executable)
            alias = Path(temporary) / 'alias'
            alias.symlink_to(executable)
            with patch.dict(os.environ, {'PATH': temporary}):
                self.assertEqual(gate.required_tool('rg'), executable)
            self.assertEqual(gate.required_tool('rg', alias), executable)

    def test_a_search_tool_containment_cannot_execute_is_named_with_its_loader_not_found_later(self):
        # A build linked against a loader the private root does not carry
        # exists on the host and cannot start inside, where exec reports the
        # file itself missing. The refusal names the loader, and a usable
        # candidate later on PATH is taken instead of it.
        with tempfile.TemporaryDirectory() as temporary:
            foreign = Path(temporary) / 'foreign'
            system = Path(temporary) / 'system'
            foreign.mkdir()
            system.mkdir()
            self.elf(foreign / 'rg', '/home/linuxbrew/.linuxbrew/lib/ld.so')
            self.elf(system / 'rg', '/lib64/ld-linux-x86-64.so.2')
            self.assertEqual(gate.elf_interpreter(foreign / 'rg'), '/home/linuxbrew/.linuxbrew/lib/ld.so')
            self.assertFalse(gate.runs_in_containment(foreign / 'rg'))
            self.assertTrue(gate.runs_in_containment(system / 'rg'))
            with patch.dict(os.environ, {'PATH': os.pathsep.join([str(foreign), str(system)])}):
                self.assertEqual(gate.required_tool('rg'), system / 'rg')
            with patch.dict(os.environ, {'PATH': str(foreign)}):
                with self.assertRaisesRegex(ValueError, r'linked against /home/linuxbrew/.linuxbrew/lib/ld.so.*--rg'):
                    gate.required_tool('rg')
            with self.assertRaisesRegex(ValueError, 'dynamically linked against /home/linuxbrew'):
                gate.required_tool('rg', foreign / 'rg')
            # Not ELF at all is left to the preflight inside, as before.
            script = Path(temporary) / 'script'
            script.write_text('#!/bin/sh\n')
            script.chmod(0o755)
            self.assertIsNone(gate.elf_interpreter(script))

    def test_preflight_executes_helpers_with_version_only_and_bounded_waits(self):
        with patch.object(gate.subprocess, 'check_output', return_value='fixture version\n') as command:
            report = gate.preflight_versions(gate.ENVIRONMENT)
        self.assertEqual(set(report), {'rustc', 'cargo', 'rg', 'bash', 'git', 'python3',
                                       'cc', 'pkg-config', 'bwrap', 'ldconfig'})
        self.assertEqual(command.call_count, 10)
        for call in command.call_args_list:
            self.assertIn(call.args[0][-1], ('--version', '-Vv'))
            self.assertEqual(call.kwargs['env'], gate.ENVIRONMENT)
            self.assertEqual(call.kwargs['timeout'], 30)
        with patch.object(gate.subprocess, 'check_output', side_effect=FileNotFoundError('missing rg')):
            with self.assertRaises(FileNotFoundError):
                gate.preflight_versions(gate.ENVIRONMENT)


    def test_verification_input_rejects_links_special_files_and_size(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            public = root / 'public'; public.write_bytes(b'certificate bytes')
            self.assertEqual(gate.verification_input(public), b'certificate bytes')
            link = root / 'alias'; link.symlink_to(public)
            fifo = root / 'pipe'; os.mkfifo(fifo)
            empty = root / 'empty'; empty.touch()
            large = root / 'large'; large.write_bytes(b'x' * (gate.MAX_PUBLIC_KEY_BYTES + 1))
            for invalid in (link, fifo, root, empty, large):
                with self.subTest(path=invalid.name):
                    with self.assertRaises((OSError, gate.VerificationError)):
                        gate.verification_input(invalid)

    def test_metadata_without_key_has_no_signature_claim(self):
        with patch.object(gate, 'verify_public_input') as verify:
            environment, report = gate.prepare_verification(True, None, gate.ENVIRONMENT,
                                                            Path('/unused'), Path('/unused'))
        self.assertEqual(environment, gate.ENVIRONMENT)
        self.assertEqual(report['status'], 'NOT_RUN')
        verify.assert_not_called()

    def check_blocked_inside(self, *, has_key, error=None, verification=None, expected_hash=None,
                             sibling_error=None):
        with tempfile.TemporaryDirectory() as temporary:
            base = Path(temporary)
            for name in ('work/source', 'work/cargo', 'work/evidence', 'usr'):
                (base / name).mkdir(parents=True, exist_ok=True)
            def private_path(value):
                return base / str(value).lstrip('/')
            with patch.object(gate, 'Path', side_effect=private_path), \
                    patch.object(gate, 'validate_entry'), patch.object(gate, 'private_hosts'), \
                    patch.object(gate, 'private_target_link'), patch.object(gate.os, 'chdir'), \
                    patch.object(gate, 'verify_public_input', side_effect=error,
                                 return_value=(gate.ENVIRONMENT, verification)) as verify, \
                    patch.object(gate, 'verify_siblings', side_effect=sibling_error,
                                 return_value=(gate.ENVIRONMENT, {})), \
                    patch.object(gate, 'preflight_versions') as versions, \
                    patch.object(gate.subprocess, 'run') as run:
                result = gate.inside(5, False, Path('/supplied-public') if has_key else None, expected_hash)
                report = json.loads((base / 'work/evidence/inner-report.json').read_text())
                self.assertEqual(result, 2)
                self.assertEqual(report['status'], 'BLOCKED')
                self.assertFalse(report['full_check_executed'])
                versions.assert_not_called(); run.assert_not_called()
                self.assertEqual(verify.call_count, int(has_key))
                return report

    def test_missing_public_key_blocks_before_canonical_command(self):
        self.check_blocked_inside(has_key=False)

    def test_signature_failure_blocks_before_canonical_command(self):
        self.check_blocked_inside(has_key=True, error=gate.VerificationError('invalid signature'))

    def test_sibling_signature_failure_blocks_before_canonical_command(self):
        self.check_blocked_inside(has_key=True, verification={'status': 'PASS'},
                                 sibling_error=gate.VerificationError('narthex commit signature failed'))

    def test_changed_public_input_identity_blocks_before_canonical_command(self):
        report = self.check_blocked_inside(has_key=True, expected_hash='0' * 64,
                                          verification={'status': 'PASS', 'public_sha256': '1' * 64})
        self.assertEqual(report['signature_verification']['status'], 'FAIL')

    def test_secret_key_packets_are_refused_before_import(self):
        for packet in (b':secret key packet:', b':secret sub key packet:'):
            with self.subTest(packet=packet), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary); public = root / 'mistaken-input'; public.write_bytes(b'fixture')
                with patch.object(gate, 'verification_command', return_value=packet) as command:
                    with self.assertRaisesRegex(gate.VerificationError, 'secret-key packets'):
                        gate.verify_public_input(public, gate.ENVIRONMENT, root / 'private-home', root)
                self.assertEqual(command.call_count, 1)
                self.assertIn('--list-packets', command.call_args.args[0])
                self.assertNotIn('--import', command.call_args.args[0])
                self.assertEqual(list((root / 'private-home').iterdir()), [])

    def test_public_import_and_signature_commands_use_only_private_home(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); public = root / 'public'; public.write_bytes(b'fixture-public')
            home = root / 'private-home'; fingerprint = 'A' * 40
            listing = ('fpr:::::::::' + fingerprint + ':\n').encode()
            responses = [b':public key packet:', b'gpg (GnuPG) fixture\n', b'imported', listing,
                         b'1' * 40 + b'\n', b'good signature', b'2' * 40 + b'\n', b'good signature']
            with patch.object(gate, 'verification_command', side_effect=responses) as command:
                environment, report = gate.verify_public_input(public, gate.ENVIRONMENT, home, root)
            self.assertEqual(report['status'], 'PASS')
            self.assertEqual(report['public_fingerprints'], [fingerprint])
            self.assertEqual(set(report['commits']), {'HEAD', 'HEAD^'})
            self.assertEqual(environment['GNUPGHOME'], str(home))
            self.assertEqual(home.stat().st_mode & 0o777, 0o700)
            self.assertNotIn('SSH_AUTH_SOCK', environment)
            self.assertNotIn('DBUS_SESSION_BUS_ADDRESS', environment)
            calls = command.call_args_list
            self.assertIn('--list-packets', calls[0].args[0])
            self.assertIn('--version', calls[1].args[0])
            self.assertIn('--import', calls[2].args[0])
            self.assertEqual(report['gpg_version'], 'gpg (GnuPG) fixture')
            for call in calls:
                self.assertEqual(call.args[1]['GNUPGHOME'], str(home))
                if call.args[0][0].endswith('/gpg'):
                    for flag in ('--no-options', '--no-autostart', '--no-auto-key-retrieve'):
                        self.assertIn(flag, call.args[0])
            self.assertIn('--no-autostart', (home / 'verify-gpg').read_text())

    def test_verification_process_deadline_and_output_bound(self):
        with self.assertRaises(gate.VerificationError):
            gate.verification_command([sys.executable, '-c', 'import time; time.sleep(5)'],
                                      gate.ENVIRONMENT, timeout=0.05)
        with patch.object(gate, 'MAX_VERIFICATION_OUTPUT', 32):
            with self.assertRaises(gate.VerificationError):
                gate.verification_command([sys.executable, '-c', 'print("x" * 65536)'],
                                          gate.ENVIRONMENT, timeout=2)

    def test_commit_reader_bounds_stream_even_if_size_preflight_lies(self):
        # The object reader is replaced by an owned fabricated process; the
        # declared size still passes. Bounds must stop stdout before validation.
        for payload, timeout in [('import sys; sys.stdout.buffer.write(b"x" * (2 * 1024 * 1024))', 2),
                                 ('import time; time.sleep(5)', .05)]:
            with self.subTest(payload=payload):
                real_command = gate.verification_command
                def fabricated(command, environment, **kwargs):
                    self.assertEqual(command[-3:], ['cat-file', 'commit', '1' * 40])
                    self.assertEqual(kwargs['output_limit'], gate.MAX_COMMIT_BYTES)
                    return real_command([sys.executable, '-c', payload], environment,
                                        timeout=timeout, output_limit=kwargs['output_limit'])
                with patch.object(gate, 'git', side_effect=['100', 'commit']), \
                        patch.object(gate, 'verification_command', side_effect=fabricated):
                    with self.assertRaisesRegex(gate.VerificationError, 'deadline or output bound'):
                        gate.commit_payload(Path('/unused'), '1' * 40)

    def test_explicit_commit_output_limit_does_not_raise_the_default_verifier_limit(self):
        payload = [sys.executable, '-c', 'import sys; sys.stdout.buffer.write(b"x" * (300 * 1024))']
        with self.assertRaises(gate.VerificationError):
            gate.verification_command(payload, gate.ENVIRONMENT)
        data = gate.verification_command(payload, gate.ENVIRONMENT, output_limit=gate.MAX_COMMIT_BYTES)
        self.assertEqual(len(data), 300 * 1024)


if __name__ == '__main__':
    unittest.main()
