#!/usr/bin/env python3
"""Run canonical checks in a private namespace with no render nodes or display.

The canonical gate attempts hardware proofs whenever render nodes are writable.
Clearing opt-in environment variables does not prevent that. This wrapper gives
the unchanged gate a fresh /dev without /dev/dri, a committed source snapshot,
and a private Cargo home containing only the explicitly mounted registry cache.
"""
import argparse
import hashlib
import json
import os
import selectors
import signal
import stat
import tempfile
import time
from pathlib import Path
import shutil
import subprocess
import sys

from isolation import ENVIRONMENT, IsolationError, Mount, launch, validate_entry

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
GIT_ENVIRONMENT = {'PATH': '/usr/bin:/bin', 'HOME': '/nonexistent',
                   'GIT_CONFIG_GLOBAL': '/dev/null', 'GIT_CONFIG_NOSYSTEM': '1',
                   'GIT_NO_REPLACE_OBJECTS': '1', 'GIT_NO_LAZY_FETCH': '1',
                   'GIT_TERMINAL_PROMPT': '0', 'LANG': 'C.UTF-8'}

SIBLINGS = ('hagia', 'narthex')
MAX_COMMIT_BYTES = 1024 * 1024
IDENTITY_CONFIG = b'[core]\n\trepositoryformatversion = 0\n\tbare = false\n'


def exact_commit(value):
    if not isinstance(value, str) or len(value) != 40 or any(c not in '0123456789abcdef' for c in value):
        raise ValueError('sibling commit must be an exact lowercase 40-hex commit, not a revision or branch')
    return value


def sibling_arguments(validate_only, values):
    selected = {}
    for name in SIBLINGS:
        source, commit = values.get(name, (None, None))
        if (source is None) != (commit is None):
            raise ValueError(f'--{name}-source and --{name}-commit must be supplied together')
        if source is None:
            if not validate_only:
                raise ValueError(f'full checks require --{name}-source and --{name}-commit')
            selected[name] = None
        else:
            selected[name] = (Path(source).resolve(strict=True), exact_commit(commit))
    return selected


def commit_payload(source, commit):
    exact_commit(commit)
    size = int(git(source, 'cat-file', '-s', commit))
    if not 0 < size <= MAX_COMMIT_BYTES or git(source, 'cat-file', '-t', commit) != 'commit':
        raise ValueError('sibling identity must name a bounded commit object')
    data = verification_command(['git', '-C', str(source), 'cat-file', 'commit', commit],
                                GIT_ENVIRONMENT, output_limit=MAX_COMMIT_BYTES, timeout=30)
    if len(data) != size or len(data) > MAX_COMMIT_BYTES:
        raise ValueError('sibling commit object changed size')
    # Verify the content-addressed identity independently, without replacements
    # or any re-encoding of the bytes covered by the commit signature.
    oid = hashlib.sha1(b'commit ' + str(len(data)).encode() + b'\0' + data).hexdigest()
    if oid != commit:
        raise ValueError('sibling commit object does not match the requested identity')
    tree = data.split(b'\n', 1)[0]
    if not tree.startswith(b'tree '):
        raise ValueError('sibling commit has no tree identity')
    return data, exact_commit(tree[5:].decode('ascii'))


def identity_repository_state(repository, commit):
    # A deliberately incomplete, commit-only identity repository. It is NOT a
    # checkout or source snapshot, and cannot satisfy consumers needing trees.
    if (repository.is_symlink() or (repository / '.git').is_symlink()
            or {p.name for p in repository.iterdir()} != {'.git'} or not (repository / '.git').is_dir()):
        raise ValueError('identity repository contains checkout files or an external Git directory')
    files = set()
    for path in (repository / '.git').rglob('*'):
        info = path.lstat()
        if stat.S_ISDIR(info.st_mode):
            continue
        if not stat.S_ISREG(info.st_mode):
            raise ValueError('identity repository contains a link or special file')
        files.add(str(path.relative_to(repository)))
    expected = {'.git/HEAD', '.git/config', '.git/shallow', f'.git/objects/{commit[:2]}/{commit[2:]}'}
    if files != expected or (repository / '.git/config').read_bytes() != IDENTITY_CONFIG:
        raise ValueError('identity repository has unexpected Git configuration or object links')
    for name in ('HEAD', 'shallow'):
        if (repository / '.git' / name).read_text() != commit + '\n':
            raise ValueError('identity repository HEAD or shallow boundary changed')
    data, tree = commit_payload(repository, commit)
    return {'commit': commit, 'tree': tree, 'commit_object_sha256': hashlib.sha256(data).hexdigest(),
            'commit_bytes': len(data), 'tree_objects_included': False, 'checkout_files': False}


def identity_repository(source, commit, destination):
    before, tree = commit_payload(source, commit)
    destination.mkdir()
    git(destination, '-c', 'init.templateDir=', 'init', '--quiet')
    (destination / '.git/config').write_bytes(IDENTITY_CONFIG)
    written = subprocess.check_output(
        ['git', '-C', str(destination), 'hash-object', '-t', 'commit', '-w', '--stdin'],
        input=before, env=GIT_ENVIRONMENT, stderr=subprocess.PIPE, timeout=30).decode().strip()
    if written != commit:
        raise ValueError('copied commit object does not match requested identity')
    (destination / '.git/HEAD').write_text(commit + '\n')
    (destination / '.git/shallow').write_text(commit + '\n')
    if commit_payload(source, commit) != (before, tree):
        raise ValueError('source commit object changed while copying identity')
    state = identity_repository_state(destination, commit)
    return {**state, 'source': str(source.resolve()), 'status': 'READY',
            'signature': 'NOT_RUN', 'source_worktree_used': False}


def prepare_siblings(selected, directory):
    directory.mkdir()
    report = {}
    for name in SIBLINGS:
        value = selected[name]
        report[name] = (identity_repository(*value, directory / name) if value else
                        {'status': 'NOT_RUN', 'reason': 'metadata-only run without explicit sibling identity'})
    (directory / 'identities.json').write_text(json.dumps(report, indent=2) + '\n')
    return report


def verify_siblings(validate_only, directory, environment, signatures):
    manifest = directory / 'identities.json'
    identities = json.loads(manifest.read_text()) if manifest.exists() else {}
    updated, result = dict(environment), {}
    for name in SIBLINGS:
        expected = identities.get(name, {'status': 'NOT_RUN'})
        if expected.get('status') == 'NOT_RUN':
            if not validate_only:
                raise VerificationError(f'full checks require explicit {name} identity')
            result[name] = {'status': 'NOT_RUN', 'reason': 'metadata-only run without explicit sibling identity'}
            continue
        if expected.get('status') != 'READY':
            raise VerificationError('invalid sibling identity manifest status')
        commit = exact_commit(expected.get('commit'))
        state = identity_repository_state(directory / name, commit)
        if any(state[key] != expected.get(key) for key in state):
            raise VerificationError(f'{name} identity changed before signature verification')
        signature = 'NOT_RUN'
        if signatures.get('status') == 'PASS':
            try:
                verification_command(['/usr/bin/git', '-C', str(directory / name), 'verify-commit', commit], environment)
            except VerificationError as error:
                raise VerificationError(f'{name} commit signature failed: {error}') from None
            signature = 'PASS'
        elif not validate_only:
            raise VerificationError('sibling signature verification requires the explicit public key')
        if identity_repository_state(directory / name, commit) != state:
            raise VerificationError(f'{name} identity changed during signature verification')
        result[name] = {**state, 'status': 'PASS', 'signature': signature,
                        'scope': 'commit identity only; tree/blob contents omitted and unverified'}
        updated[f'SOPHIA_{name.upper()}_ROOT'] = str(directory / name)
    return updated, result


def unchanged_siblings(identities, directory):
    for name, expected in identities.items():
        if expected['status'] != 'PASS':
            continue
        state = identity_repository_state(directory / name, expected['commit'])
        if any(state[key] != expected[key] for key in state):
            raise VerificationError(f'{name} identity changed during contained command')


def git(source, *arguments):
    return subprocess.check_output(['git', '-C', str(source), *arguments],
                                   env=GIT_ENVIRONMENT, stderr=subprocess.PIPE,
                                   timeout=120).decode().strip()


def source_state(source):
    return {'commit': git(source, 'rev-parse', 'HEAD'),
            'tree': git(source, 'rev-parse', 'HEAD^{tree}'),
            'dirty': bool(git(source, 'status', '--porcelain=v1', '--untracked-files=all'))}


def archive_digest(source, commit):
    digest = hashlib.sha256()
    with subprocess.Popen(['git', '-C', str(source), 'archive', '--format=tar', commit],
                          env=GIT_ENVIRONMENT, stdout=subprocess.PIPE,
                          stderr=subprocess.DEVNULL) as process:
        while chunk := process.stdout.read(1024 * 1024):
            digest.update(chunk)
        if process.wait(timeout=120):
            raise RuntimeError('could not hash committed source archive')
    return digest.hexdigest()


def snapshot(source, destination):
    before = source_state(source)
    if before['dirty']:
        raise ValueError('source is dirty; commit changes before taking a canonical snapshot')
    destination.mkdir()
    git(destination, '-c', 'init.templateDir=', 'init', '--quiet')
    # Fetch an exact local object, not a branch name, configured remote or shared
    # worktree. The shallow repository owns its object data and index.
    git(destination, '-c', 'protocol.file.allow=always', 'fetch', '--quiet',
        '--no-tags', '--depth=2', str(source.resolve()), before['commit'])
    git(destination, '-c', 'advice.detachedHead=false', 'checkout', '--quiet',
        '--detach', before['commit'])
    if source_state(source) != before:
        raise ValueError('source HEAD or working state changed while taking the snapshot')
    if source_state(destination) != before:
        raise ValueError('snapshot does not match committed source')
    if (destination / '.git/objects/info/alternates').exists() or (destination / '.git/commondir').exists():
        raise ValueError('snapshot has an external Git object/worktree dependency')
    return {**before, 'source': str(source.resolve()),
            'archive_sha256': archive_digest(destination, before['commit'])}


def file_digest(path):
    digest = hashlib.sha256()
    with path.open('rb') as source:
        while chunk := source.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def toolchain_path(source):
    environment = {'PATH': os.environ.get('PATH', '/usr/bin:/bin'),
                   'HOME': str(Path.home())}
    result = subprocess.check_output(['rustc', '--print', 'sysroot'], cwd=source,
                                     env=environment, timeout=30, stderr=subprocess.PIPE)
    return Path(result.decode().strip()).resolve(strict=True)


def elf_interpreter(path):
    """The program interpreter an ELF executable asks for, or None.

    None for a statically linked executable and for anything that is not
    ELF; the caller decides what that means. Reads only the headers."""
    with open(path, 'rb') as handle:
        header = handle.read(64)
        if header[:4] != b'\x7fELF' or header[4] != 2 or header[5] != 1:
            return None
        phoff = int.from_bytes(header[32:40], 'little')
        phentsize = int.from_bytes(header[54:56], 'little')
        phnum = int.from_bytes(header[56:58], 'little')
        for index in range(phnum):
            handle.seek(phoff + index * phentsize)
            entry = handle.read(phentsize)
            if int.from_bytes(entry[:4], 'little') != 3:  # PT_INTERP
                continue
            offset = int.from_bytes(entry[8:16], 'little')
            size = int.from_bytes(entry[32:40], 'little')
            handle.seek(offset)
            return handle.read(size).split(b'\0', 1)[0].decode(errors='replace')
    return None


def runs_in_containment(path):
    """Whether the container can execute this binary: static, or dynamically
    linked against the system loader the private root carries. A binary
    linked against another loader (a linuxbrew build, for one) exists on the
    host and cannot start inside, where exec reports the file itself missing."""
    interpreter = elf_interpreter(path)
    return interpreter is None or interpreter.startswith(('/lib', '/usr/lib'))


def required_tool(name, explicit=None):
    """The executable the container will run for `name`: the explicit path
    if one was given, else the first on PATH that containment can execute.
    Every candidate refused is named with the loader it asks for, so a
    missing or unusable helper is a blocker with its reason, not a search
    result that quietly changes the verdict."""
    if explicit is not None:
        path = Path(explicit).resolve(strict=True)
        if not runs_in_containment(path):
            raise ValueError(f'{name} at {path} is dynamically linked against '
                             f'{elf_interpreter(path)}, which containment cannot provide')
        return path
    refused = []
    for directory in os.environ.get('PATH', '').split(os.pathsep):
        candidate = Path(directory or '.') / name
        if not (candidate.is_file() and os.access(candidate, os.X_OK)):
            continue
        path = candidate.resolve(strict=True)
        if runs_in_containment(path):
            return path
        refused.append(f'{path} (linked against {elf_interpreter(path)})')
    if refused:
        raise ValueError(f'no {name} on PATH runs in containment; refused '
                         + ', '.join(refused)
                         + f'; install a system-linked or static {name}, or pass --{name}')
    raise ValueError(f'missing canonical check dependency: {name}')


def check_paths(source, output, target):
    common = Path(git(source, 'rev-parse', '--path-format=absolute', '--git-common-dir'))
    artifacts = (common.parent / '.artifacts').resolve()
    output, target = output.resolve(), target.resolve()
    for name, path in (('output', output), ('target', target)):
        if (path == artifacts or not path.is_relative_to(artifacts)
                or path.is_relative_to('/tmp')):
            raise ValueError(f'{name} must be a disk-backed child of {artifacts}')
    if output.is_relative_to(target) or target.is_relative_to(output):
        raise ValueError('output and target paths must not overlap')
    if output.exists():
        raise ValueError('output must be new; retained evidence is immutable')
    return output, target


def private_hosts(root):
    # Renderer refusal tests use /etc/hosts solely as a regular, non-device
    # file. Supply private runtime data instead of exposing the host's /etc.
    directory = root / 'etc'
    directory.mkdir(exist_ok=True)
    (directory / 'hosts').write_text('127.0.0.1 localhost\n::1 localhost\n')


def private_target_link(source):
    # Existing canonical profile readers locate binaries relative to the
    # source tree, independently of Cargo's target-dir setting. Link only this
    # private copy to the explicitly mounted disk target, never an installation.
    (source / 'target').symlink_to('/work/target', target_is_directory=True)


def private_loader_cache():
    # Nested production protection domains bind this exact path. Generate it
    # from private, already-allowlisted libraries; never copy host /etc or let
    # ldconfig read host configuration or modify library symlinks.
    subprocess.run(['/usr/bin/ldconfig', '-X', '-i', '-f', '/dev/null',
                    '-C', '/etc/ld.so.cache', '/usr/lib'], env=ENVIRONMENT,
                   stdin=subprocess.DEVNULL, capture_output=True, text=True,
                   check=True, timeout=30)


def preflight_versions(environment):
    # audit_source_layout.sh currently swallows a missing rg in a conditional:
    # the retained 52d20c5d run falsely reported eleven inline-test debts retired.
    # Refuse missing/non-executable helpers before paying for the full suite.
    commands = {
        'rustc': ['/work/toolchain/bin/rustc', '-Vv'],
        'cargo': ['/work/toolchain/bin/cargo', '-Vv'],
        'rg': ['/work/tools/rg', '--version'],
        **{tool: [f'/usr/bin/{tool}', '--version'] for tool in
           ('bash', 'git', 'python3', 'cc', 'pkg-config', 'bwrap', 'ldconfig')},
    }
    return {name: subprocess.check_output(command, env=environment, text=True,
                                         stderr=subprocess.STDOUT, timeout=30).strip()
            for name, command in commands.items()}



MAX_PUBLIC_KEY_BYTES = 64 * 1024
MAX_VERIFICATION_OUTPUT = 256 * 1024


class VerificationError(ValueError):
    pass


def verification_input(path):
    # The caller selects one export, never a directory or host keyring mount.
    # Refuse links and special files before opening (notably blocking FIFOs).
    descriptor = os.open(path, os.O_RDONLY | os.O_NONBLOCK | os.O_NOFOLLOW)
    try:
        info = os.fstat(descriptor)
        if not stat.S_ISREG(info.st_mode) or not 0 < info.st_size <= MAX_PUBLIC_KEY_BYTES:
            raise VerificationError('verification input must be a bounded nonempty regular file')
        with os.fdopen(os.dup(descriptor), 'rb') as source:
            data = source.read(MAX_PUBLIC_KEY_BYTES + 1)
        if not 0 < len(data) <= MAX_PUBLIC_KEY_BYTES:
            raise VerificationError('verification input changed size or exceeds the limit')
        return data
    finally:
        os.close(descriptor)


def verification_command(command, environment, *, payload=b'', timeout=30, output_limit=None):
    # Raw packet diagnostics can contain private material in a mistaken input.
    # Keep them only in bounded memory, never an evidence log. The input copy is
    # anonymous in private tmpfs; only an accepted public import persists.
    limit = MAX_VERIFICATION_OUTPUT if output_limit is None else output_limit
    if not isinstance(limit, int) or limit <= 0:
        raise VerificationError('verification output limit must be positive')
    with tempfile.TemporaryFile() as incoming:
        incoming.write(payload)
        incoming.seek(0)
        with subprocess.Popen(command, env=environment, stdin=incoming,
                              stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                              close_fds=True, start_new_session=True) as child:
            deadline = time.monotonic() + timeout
            output = bytearray()
            try:
                os.set_blocking(child.stdout.fileno(), False)
                with selectors.DefaultSelector() as selector:
                    selector.register(child.stdout, selectors.EVENT_READ)
                    while selector.get_map():
                        remaining = deadline - time.monotonic()
                        if remaining <= 0:
                            raise VerificationError('verification command timed out')
                        if not selector.select(remaining):
                            raise VerificationError('verification command timed out')
                        chunk = os.read(child.stdout.fileno(), min(16384, limit - len(output) + 1))
                        if not chunk:
                            selector.unregister(child.stdout)
                        else:
                            output.extend(chunk)
                            if len(output) > limit:
                                raise VerificationError('verification output limit exceeded')
                child.wait(timeout=max(0.001, deadline - time.monotonic()))
            except (VerificationError, subprocess.TimeoutExpired):
                os.killpg(child.pid, signal.SIGKILL)
                child.wait()
                raise VerificationError('verification deadline or output bound exceeded') from None
            if child.returncode:
                raise VerificationError('verification command refused the input or signature')
            return bytes(output)


def verify_public_input(key_path, environment, directory, source):
    data = verification_input(key_path)
    directory.mkdir(mode=0o700)
    private_environment = {**environment, 'GNUPGHOME': str(directory)}
    flags = ['/usr/bin/gpg', '--batch', '--no-options', '--no-autostart',
             '--no-auto-key-retrieve', '--no-auto-check-trustdb']
    packets = verification_command([*flags, '--list-packets'], private_environment, payload=data)
    if b':secret key packet:' in packets or b':secret sub key packet:' in packets:
        raise VerificationError('secret-key packets are forbidden; nothing was imported')
    if b':public key packet:' not in packets:
        raise VerificationError('verification input contains no public key certificate')
    version = verification_command([*flags, '--version'], private_environment).decode('utf-8', errors='replace').splitlines()
    if not version:
        raise VerificationError('GnuPG returned no version identity')
    verification_command([*flags, '--import-options', 'import-minimal', '--import'],
                         private_environment, payload=data)
    listing = verification_command([*flags, '--with-colons', '--with-subkey-fingerprint', '--fingerprint', '--list-keys'],
                                   private_environment).decode('utf-8', errors='replace')
    fingerprints = sorted({line.split(':')[9] for line in listing.splitlines()
                           if line.startswith('fpr:') and len(line.split(':')) > 9})
    if not fingerprints or any(len(value) not in (40, 64) or
                               any(letter not in '0123456789ABCDEF' for letter in value)
                               for value in fingerprints):
        raise VerificationError('public import returned no valid fingerprint inventory')
    helper = directory / 'verify-gpg'
    helper.write_text('#!/bin/sh\nexec /usr/bin/gpg --batch --no-options --no-autostart '
                      '--no-auto-key-retrieve --no-auto-check-trustdb "$@"\n')
    helper.chmod(0o700)
    private_environment.update(GIT_CONFIG_COUNT='1', GIT_CONFIG_KEY_0='gpg.program',
                               GIT_CONFIG_VALUE_0=str(helper))
    commits = {}
    for revision in ('HEAD', 'HEAD^'):
        commit = verification_command(['/usr/bin/git', '-C', str(source), 'rev-parse', revision],
                                      private_environment).decode().strip()
        if len(commit) != 40 or any(letter not in '0123456789abcdef' for letter in commit):
            raise VerificationError('snapshot is missing required commit history')
        verification_command(['/usr/bin/git', '-C', str(source), 'verify-commit', commit],
                             private_environment)
        commits[revision] = {'commit': commit, 'signature': 'PASS'}
    return private_environment, {'status': 'PASS', 'public_sha256': hashlib.sha256(data).hexdigest(),
                                 'public_bytes': len(data), 'public_fingerprints': fingerprints,
                                 'gpg_version': version[0],
                                 'commits': commits, 'host_ownertrust_imported': False}


def prepare_verification(validate_only, key_path, environment, directory, source):
    if key_path is None:
        if not validate_only:
            raise VerificationError('full checks require --verification-key with public signing keys')
        return environment, {'status': 'NOT_RUN', 'reason': 'metadata-only run without a verification key'}
    return verify_public_input(key_path, environment, directory, source)


def inside(activation_fd, validate_only, verification_key=None, verification_sha256=None):
    validate_entry(activation_fd)  # Before changing environment, directories or running tools.
    if Path('/dev/dri').exists():
        raise IsolationError('render-device namespace is not empty')
    private_hosts(Path('/'))
    private_target_link(Path('/work/source'))
    Path('/work/cargo/registry').symlink_to('/work/registry', target_is_directory=True)
    Path('/usr/include').symlink_to('/work/include', target_is_directory=True)
    environment = {**ENVIRONMENT, 'PATH': '/work/toolchain/bin:/work/tools:/usr/bin:/bin',
                   'CARGO': '/work/toolchain/bin/cargo',
                   'RUSTC': '/work/toolchain/bin/rustc',
                   'RUSTDOC': '/work/toolchain/bin/rustdoc',
                   'CARGO_HOME': '/work/cargo', 'CARGO_TARGET_DIR': '/work/target',
                   'CARGO_NET_OFFLINE': 'true', 'GIT_CONFIG_GLOBAL': '/dev/null',
                   'GIT_CONFIG_NOSYSTEM': '1', 'GIT_TERMINAL_PROMPT': '0',
                   'GIT_NO_REPLACE_OBJECTS': '1', 'GIT_NO_LAZY_FETCH': '1',
                   'PWD': '/work/source'}
    os.chdir('/work/source')
    signatures = {'status': 'NOT_RUN', 'reason': 'signature preflight has not completed'}
    siblings = {'status': 'NOT_RUN', 'reason': 'sibling identity preflight has not completed'}
    try:
        environment, signatures = prepare_verification(
            validate_only, verification_key, environment,
            Path('/work/evidence/verification-home'), Path('/work/source'))
        if verification_sha256 is not None and signatures.get('public_sha256') != verification_sha256:
            signatures = {'status': 'FAIL', 'detail': 'verification input changed before private import'}
            raise VerificationError(signatures['detail'])
        Path('/work/evidence/signature-verification.json').write_text(json.dumps(signatures, indent=2) + '\n')
        environment, identities = verify_siblings(
            validate_only, Path('/work/dependencies'), environment, signatures)
        siblings = {'status': 'PASS' if all(value['status'] == 'PASS' for value in identities.values())
                    else 'NOT_RUN', 'identities': identities}
        Path('/work/evidence/sibling-identities.json').write_text(json.dumps(siblings, indent=2) + '\n')
        versions = preflight_versions(environment)
        private_loader_cache()
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        if verification_key is not None and signatures['status'] == 'NOT_RUN':
            signatures = {'status': 'FAIL', 'detail': str(error)}
        if signatures['status'] == 'PASS' and 'identities' not in siblings:
            siblings = {'status': 'FAIL', 'detail': str(error)}
        report = {'status': 'BLOCKED', 'full_check_executed': False,
                  'detail': f'private preflight failed: {type(error).__name__}: {error}',
                  'signature_verification': signatures, 'sibling_prerequisites': siblings}
        Path('/work/evidence/inner-report.json').write_text(json.dumps(report, indent=2) + '\n')
        return 2
    Path('/work/evidence/preflight.json').write_text(json.dumps(versions, indent=2) + '\n')
    command = ['/work/toolchain/bin/cargo', 'metadata', '--offline', '--locked',
               '--format-version', '1'] if validate_only else ['/work/toolchain/bin/cargo', 'xtask', 'check']
    with Path('/work/evidence/command.log').open('wb') as log:
        result = subprocess.run(command, env=environment, stdin=subprocess.DEVNULL,
                                stdout=log, stderr=subprocess.STDOUT, check=False)
    identity_failure = None
    try:
        unchanged_siblings(identities, Path('/work/dependencies'))
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        identity_failure = str(error)
    report = {'status': 'PASS' if result.returncode == 0 else 'FAIL',
              'command': command, 'command_exit': result.returncode, 'tool_versions': versions,
              'full_check_executed': not validate_only, 'signature_verification': signatures,
              'sibling_prerequisites': siblings,
              'scope': 'Contained offline checks; no hardware or physical-input acceptance.',
              'render_devices_present': False,
              'private_runtime_data': ['generated /etc/hosts: loopback names only',
                                       'generated /etc/ld.so.cache: private allowlisted libraries only',
                                       'private source/target symlink to owned /work/target'],
              'hardware_proofs': {'status': 'NOT_RUN', 'reason': 'private /dev has no render nodes'},
              'promoted_host_archives': {'status': 'NOT_RUN', 'reason': 'host state directories are not mounted'}}
    if identity_failure:
        report.update(status='FAIL', detail=identity_failure)
        siblings['status'] = 'FAIL'
    Path('/work/evidence/inner-report.json').write_text(json.dumps(report, indent=2) + '\n')
    return result.returncode or int(identity_failure is not None)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source', type=Path, default=ROOT)
    parser.add_argument('--output', type=Path)
    parser.add_argument('--target-dir', type=Path)
    parser.add_argument('--registry', type=Path, default=Path.home() / '.cargo/registry')
    parser.add_argument('--verification-key', type=Path, help='one bounded public OpenPGP export; required for full checks')
    for sibling in SIBLINGS:
        parser.add_argument(f'--{sibling}-source', type=Path, help='explicit repository containing the pinned commit; never mounted')
        parser.add_argument(f'--{sibling}-commit', help='exact lowercase 40-hex signed commit identity')
    parser.add_argument('--validate-only', action='store_true', help='versions, optional signature preflight and offline metadata only; no check/build/test')
    parser.add_argument('--timeout', type=float, default=1800)
    parser.add_argument('--rg', type=Path, help='the ripgrep executable to mount; default: the first on PATH that containment can execute')
    parser.add_argument('--inside', action='store_true', help=argparse.SUPPRESS)
    parser.add_argument('--verification-sha256', help=argparse.SUPPRESS)
    parser.add_argument('--activation-fd', type=int, default=-1, help=argparse.SUPPRESS)
    args = parser.parse_args()
    if args.inside:
        return inside(args.activation_fd, args.validate_only, args.verification_key, args.verification_sha256)
    if not args.output or not args.target_dir:
        parser.error('--output and --target-dir are required')
    if not 0 < args.timeout <= 1800:
        parser.error('timeout must be in (0, 1800]')
    source = args.source.resolve(strict=True)
    if source_state(source)['dirty']:
        parser.error('source is dirty; no canonical snapshot was taken')
    output, target = check_paths(source, args.output, args.target_dir)
    output.mkdir(parents=True)
    try:
        selected = sibling_arguments(args.validate_only, {
            name: (getattr(args, f'{name}_source'), getattr(args, f'{name}_commit')) for name in SIBLINGS})
    except (OSError, ValueError) as error:
        report = {'status': 'BLOCKED', 'full_check_executed': False,
                  'detail': f'invalid sibling prerequisites: {error}'}
        (output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
        print(json.dumps(report, indent=2))
        return 2
    toolchain = toolchain_path(source)
    try:
        ripgrep = required_tool('rg', args.rg)
    except (OSError, ValueError) as error:
        report = {'status': 'BLOCKED', 'full_check_executed': False,
                  'detail': f'canonical check dependency: {error}'}
        (output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
        print(json.dumps(report, indent=2))
        return 2
    tools = ('rustc', 'rustdoc', 'cargo', 'cargo-fmt', 'rustfmt', 'cargo-clippy', 'clippy-driver')
    tool_hashes = {name: file_digest(toolchain / 'bin' / name) for name in tools}
    registry = args.registry.resolve(strict=True)
    if not all((registry / part).is_dir() for part in ('cache', 'index', 'src')):
        parser.error('registry must contain offline cache, index and unpacked src directories')
    provenance = snapshot(source, output / 'source')
    try:
        sibling_provenance = prepare_siblings(selected, output / 'dependencies')
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        report = {'status': 'BLOCKED', 'full_check_executed': False,
                  'detail': f'sibling identity copy failed: {type(error).__name__}: {error}',
                  'provenance': provenance}
        (output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
        print(json.dumps(report, indent=2))
        return 2
    for directory in ('evidence', 'cargo'):
        (output / directory).mkdir()
    target.mkdir(parents=True, exist_ok=True)
    key_input = None
    if args.verification_key is not None:
        try:
            data = verification_input(args.verification_key)
            key_input = {'source': str(args.verification_key.absolute()),
                         'sha256': hashlib.sha256(data).hexdigest(), 'bytes': len(data)}
        except (OSError, ValueError) as error:
            report = {'status': 'BLOCKED', 'full_check_executed': False,
                      'detail': f'invalid verification input: {error}', 'provenance': provenance}
            (output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
            print(json.dumps(report, indent=2))
            return 2
    mounts = [Mount(HERE, '/work/harness/tools/probes/x11_conformance'),
              Mount(output / 'source', '/work/source', writable=True),
              Mount(output / 'cargo', '/work/cargo', writable=True),
              Mount(output / 'evidence', '/work/evidence', writable=True),
              Mount(target, '/work/target', writable=True), Mount(toolchain, '/work/toolchain'),
              Mount(ripgrep, '/work/tools/rg'),
              Mount(registry, '/work/registry'), Mount(Path('/usr/include'), '/work/include')]
    mounts.append(Mount(output / 'dependencies/identities.json', '/work/dependencies/identities.json'))
    for name in SIBLINGS:
        if selected[name] is not None:
            mounts.append(Mount(output / 'dependencies' / name, f'/work/dependencies/{name}'))
    command = ['/usr/bin/python3', '-B',
               '/work/harness/tools/probes/x11_conformance/offline_check.py', '--inside',
               '--activation-fd', '{activation_fd}']
    if args.verification_key is not None:
        mounts.append(Mount(args.verification_key.absolute(), '/work/verification/public-key'))
        command += ['--verification-key', '/work/verification/public-key',
                    '--verification-sha256', key_input['sha256']]
    if args.validate_only:
        command.append('--validate-only')
    provenance.update(toolchain=str(toolchain), tool_sha256=tool_hashes,
                      auxiliary_tools={'rg': {'source': str(ripgrep), 'sha256': file_digest(ripgrep)}},
                      wrapper_sha256=file_digest(Path(__file__)),
                      isolation_sha256=file_digest(HERE / 'isolation.py'),
                      target=str(target), registry=str(registry), validate_only=args.validate_only,
                      sibling_inputs=sibling_provenance,
                      public_verification_input=key_input)
    (output / 'provenance.json').write_text(json.dumps(provenance, indent=2) + '\n')
    try:
        result = launch(command, mounts=mounts, timeout=args.timeout)
        (output / 'adapter.log').write_bytes(result.stdout + result.stderr)
        inner = output / 'evidence/inner-report.json'
        report = json.loads(inner.read_text()) if inner.exists() else {
            'status': 'NORESULT', 'full_check_executed': False if args.validate_only else None,
            'detail': 'contained command produced no completion report; see adapter.log'}
        expected_block = (result.returncode == 2 and report.get('status') == 'BLOCKED'
                          and report.get('full_check_executed') is False)
        if result.returncode and not expected_block:
            report['status'] = 'TIMEOUT' if result.returncode == 124 else 'FAIL'
            report['adapter_exit'] = result.returncode
    except IsolationError as error:
        report = {'status': 'BLOCKED', 'full_check_executed': False, 'detail': str(error)}
    signatures = report.get('signature_verification', {'status': 'NOT_RUN'})
    try:
        for name, expected in sibling_provenance.items():
            if expected['status'] != 'READY':
                continue
            copied = identity_repository_state(output / 'dependencies' / name, expected['commit'])
            raw, tree = commit_payload(selected[name][0], expected['commit'])
            if (any(copied[key] != expected[key] for key in copied)
                    or tree != expected['tree'] or hashlib.sha256(raw).hexdigest() != expected['commit_object_sha256']):
                raise ValueError(f'{name} source or copied commit changed during the run')
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        report.update(status='FAIL', detail=f'sibling identity integrity check failed: {error}')
    if key_input and signatures.get('status') == 'PASS' and signatures.get('public_sha256') != key_input['sha256']:
        report['status'] = 'FAIL'
        report['detail'] = 'verification input changed between launch provenance and private import'
    provenance['signature_verification'] = signatures
    (output / 'provenance.json').write_text(json.dumps(provenance, indent=2) + '\n')
    report['provenance'] = provenance
    report['hardware_proofs'] = {'status': 'NOT_RUN', 'reason': 'render nodes are never mounted'}
    (output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report, indent=2))
    return 0 if report['status'] == 'PASS' else 2 if report['status'] == 'BLOCKED' else 1


if __name__ == '__main__':
    raise SystemExit(main())
