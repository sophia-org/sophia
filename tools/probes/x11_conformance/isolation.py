"""Private-instance launch boundary; no ambient display or authorization fallback.

The runner is trusted to choose artifact mounts and delegated capabilities. A
pathname or UID is not authorization. validate_entry prevents accidental direct
entry and checks the launch contract; it is not attestation against a hostile
same-UID process capable of constructing its own namespaces.
"""
from dataclasses import dataclass
from contextlib import ExitStack, contextmanager
import fcntl
import json
import os
from pathlib import Path
import shutil
import signal
import stat
import subprocess


NAMESPACES = ('mnt', 'net', 'pid', 'user', 'ipc', 'uts')
NAMESPACE_TYPES = dict(zip(NAMESPACES, (0x20000, 0x40000000, 0x20000000,
                                      0x10000000, 0x8000000, 0x4000000)))
ENVIRONMENT = {'PATH': '/usr/bin:/bin', 'HOME': '/home/test', 'LANG': 'C.UTF-8',
               'PYTHONDONTWRITEBYTECODE': '1', 'PWD': '/work'}


class IsolationError(RuntimeError):
    pass


@dataclass(frozen=True)
class Mount:
    source: Path
    destination: str
    writable: bool = False


def namespace_ids():
    return {name: os.readlink(f'/proc/self/ns/{name}') for name in NAMESPACES}


def fd_identity(fd):
    value = os.fstat(fd)
    return [value.st_dev, value.st_ino, stat.S_IFMT(value.st_mode)]


@contextmanager
def _activation(delegated_fds, relayed):
    # Actual namespace descriptors prevent a forged JSON namespace-name string
    # from authorizing an unsandboxed --inside invocation.
    #
    # ONE CROSSING, ONE PAYLOAD PER ENTRY. A relayed activation is for an entry
    # the first one starts inside the same namespaces, which needs its own
    # validation rather than the first vouching for it. Every payload records
    # these namespaces, so each entry proves the same crossing independently.
    # They differ only in the descriptors each is given: the first is told
    # about everything that crosses, because its own scan refuses an inherited
    # descriptor it was not told about, and each relayed entry is told only
    # about its own.
    with ExitStack() as stack:
        # A SET PER ENTRY, NOT ONE SHARED. Validation closes the namespace
        # descriptors it was given, so entries sharing a set would leave the
        # second validating against descriptors the first had already closed.
        entries = []
        for _ in range(len(relayed) + 1):
            reader, writer = os.pipe2(os.O_CLOEXEC)
            stack.callback(os.close, reader)
            entries.append((reader, writer,
                            {name: stack.enter_context(open(f'/proc/self/ns/{name}', 'rb')).fileno()
                             for name in NAMESPACES}))
        readers = tuple(reader for reader, _, _ in entries)
        # The first entry's own activation pipe and namespace descriptors are
        # consumed by its validation, so they are not among the descriptors it
        # is told it holds. Everything meant for a later entry is, because its
        # scan refuses an inherited descriptor it was not told about.
        carried = (tuple(delegated_fds) + readers[1:]
                   + tuple(fd for group in relayed for fd in group)
                   + tuple(fd for _, _, held in entries[1:] for fd in held.values()))
        for (_, writer, namespaces), granted in zip(entries, (carried, *relayed)):
            payload = {'namespaces': namespaces,
                       'descriptors': {str(fd): fd_identity(fd) for fd in granted}}
            encoded = json.dumps(payload).encode()
            if len(encoded) > 4096:
                raise IsolationError('too many delegated descriptors')
            try:
                os.write(writer, encoded)
            finally:
                os.close(writer)
        # What each relayed entry must inherit to validate and work: its own
        # activation pipe, the namespace descriptors that prove its crossing,
        # and the capabilities it was granted. The first entry passes exactly
        # this on, rather than guessing from what it holds.
        inheritance = tuple((reader, *held.values(), *group)
                            for (reader, _, held), group in zip(entries[1:], relayed))
        yield (readers, tuple(fd for _, _, held in entries for fd in held.values()),
               carried, inheritance)


def _validate_mount(mount):
    source = Path(mount.source).resolve(strict=True)
    destination = Path(mount.destination)
    if (not destination.is_absolute() or '..' in destination.parts
            or not destination.is_relative_to('/work') or destination == Path('/work')):
        raise IsolationError('artifacts must map beneath /work')
    if not (source.is_file() or source.is_dir()):
        raise IsolationError('artifact is not a regular file or directory')
    # Never authorize sockets or devices by treating them as artifact data.
    paths = [source]
    if source.is_dir():
        paths.extend(source.rglob('*'))
    for path in paths:
        mode = path.lstat().st_mode
        if not (stat.S_ISREG(mode) or stat.S_ISDIR(mode) or stat.S_ISLNK(mode)):
            raise IsolationError(f'non-data artifact: {path}')
    return source


def command(bwrap, argv, mounts, activation_fds, relayed_fds=()):
    """Build a confined command; use launch() to enforce FD/environment hygiene.

    The literal argument {activation_fd} is replaced with the runner-owned pipe.
    Inner Python entrypoints must validate_entry(int(argument)) before doing work.
    A relayed activation is named {activation_fd_2}, {activation_fd_3} and so
    on, in the order it was requested; the entry passes one to the process it
    starts rather than validating it, along with every descriptor named by the
    matching {relayed_fds_2}, which that process must inherit exactly.
    """
    if not argv or not Path(argv[0]).is_absolute():
        raise IsolationError('inner executable must be absolute')
    result = [str(bwrap), '--die-with-parent', '--unshare-all', '--new-session',
              '--cap-drop', 'ALL', '--clearenv', '--tmpfs', '/', '--dir', '/usr']
    for part in ('bin', 'lib', 'share'):
        path = Path('/usr') / part
        if path.is_dir():
            result += ['--ro-bind', str(path), str(path)]
    result += ['--symlink', 'usr/bin', '/bin', '--symlink', 'usr/lib', '/lib',
               '--symlink', 'usr/lib', '/lib64', '--symlink', 'lib', '/usr/lib64',
               '--tmpfs', '/tmp', '--tmpfs', '/run', '--tmpfs', '/home',
               '--dir', '/home/test', '--dir', '/work', '--proc', '/proc',
               '--dev', '/dev', '--chdir', '/work']
    destinations = []
    for mount in mounts:
        source = _validate_mount(mount)
        destination = Path(mount.destination)
        if any(destination.is_relative_to(other) or other.is_relative_to(destination)
               for other in destinations):
            raise IsolationError('artifact mounts must not overlap')
        destinations.append(destination)
        result += ['--bind' if mount.writable else '--ro-bind', str(source), str(destination)]
    for name, value in ENVIRONMENT.items():
        result += ['--setenv', name, value]
    activation_fds = ((activation_fds,) if isinstance(activation_fds, int)
                      else tuple(activation_fds))
    named = {'{activation_fd}': str(activation_fds[0])}
    for position, fd in enumerate(activation_fds[1:], start=2):
        named[f'{{activation_fd_{position}}}'] = str(fd)
    for position, group in enumerate(relayed_fds, start=2):
        named[f'{{relayed_fds_{position}}}'] = ','.join(str(fd) for fd in group)
    return result + ['--'] + [named.get(str(arg), str(arg)) for arg in argv]


def launch(argv, *, mounts=(), delegated_fds=(), timeout=30, bwrap=None,
           relayed_activations=()):
    """Run an owned private process group with exact descriptor inheritance.

    Returns CompletedProcess (124 on absolute timeout). No subprocess stdin or
    ambient sockets are inherited. Delegated descriptors are intentional runner
    capabilities, and are kept open after entry validation for its caller.

    `relayed_activations` is one tuple of descriptors per additional entry the
    inner process will start beside itself, in the same namespaces. Each gets
    its own activation pipe describing this crossing and naming only those
    descriptors, so it validates its own entry rather than being vouched for.
    """
    if not 0 < timeout <= 1800:
        raise IsolationError('timeout must be in (0, 1800]')
    bwrap = bwrap or shutil.which('bwrap')
    if not bwrap:
        raise IsolationError('BLOCKED: bubblewrap is unavailable')
    delegated_fds = tuple(delegated_fds)
    relayed = tuple(tuple(group) for group in relayed_activations)
    crossing = delegated_fds + tuple(fd for group in relayed for fd in group)
    if len(set(crossing)) != len(crossing) or any(fd < 3 for fd in crossing):
        raise IsolationError('delegated descriptors must be unique and above stderr')
    with _activation(delegated_fds, relayed) as (readers, namespace_fds, _carried, inheritance):
        arguments = command(bwrap, argv, mounts, readers, inheritance)
        with subprocess.Popen(arguments, env=ENVIRONMENT, stdin=subprocess.DEVNULL,
                              stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                              close_fds=True, pass_fds=(*readers, *namespace_fds, *crossing),
                              start_new_session=True) as process:
            try:
                stdout, stderr = process.communicate(timeout=timeout)
                status = process.returncode
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                stdout, stderr = process.communicate()
                status = 124
        return subprocess.CompletedProcess(arguments, status, stdout, stderr)


def validate_entry(activation_fd):
    """Consume runner activation and validate isolation before creating clients.

    Returns the set of explicitly delegated descriptors. Fails closed; there is
    deliberately no ambient-authorization callback or fallback parameter.
    """
    try:
        if activation_fd < 3 or not stat.S_ISFIFO(os.fstat(activation_fd).st_mode):
            raise IsolationError('activation must be a runner pipe')
        os.set_blocking(activation_fd, False)
        payload = os.read(activation_fd, 4097)
        if len(payload) > 4096:
            raise IsolationError('oversized activation')
        record = json.loads(payload)
    except (OSError, ValueError, TypeError) as error:
        raise IsolationError('missing or invalid runner activation') from error
    finally:
        if activation_fd >= 3:
            try:
                os.close(activation_fd)
            except OSError:
                pass
    if not isinstance(record, dict) or set(record) != {'namespaces', 'descriptors'}:
        raise IsolationError('invalid activation schema')
    actual = namespace_ids()
    previous = record['namespaces']
    if (not isinstance(previous, dict) or set(previous) != set(NAMESPACES)
            or any(type(fd) is not int or fd < 3 for fd in previous.values())
            or len(set(previous.values())) != len(NAMESPACES)):
        raise IsolationError('invalid namespace descriptor record')
    try:
        for name, fd in previous.items():
            # Linux nsfs NS_GET_NSTYPE, not caller-provided provenance.
            if (fcntl.ioctl(fd, 0xb703) != NAMESPACE_TYPES[name]
                    or os.readlink(f'/proc/self/fd/{fd}') == actual[name]):
                raise IsolationError('entry has not crossed every required namespace')
    except OSError as error:
        raise IsolationError('invalid kernel namespace evidence') from error
    finally:
        for fd in previous.values():
            try:
                os.close(fd)
            except OSError:
                pass
    if dict(os.environ) != ENVIRONMENT:
        raise IsolationError('ambient environment present')
    try:
        with open('/dev/tty', 'rb', buffering=0):
            raise IsolationError('controlling terminal present')
    except OSError:
        pass
    if not isinstance(record['descriptors'], dict):
        raise IsolationError('invalid descriptor record')
    delegated = {int(fd): identity for fd, identity in record['descriptors'].items()}
    for fd, identity in delegated.items():
        if fd < 3 or fd_identity(fd) != identity:
            raise IsolationError('delegated capability changed')
    for name in os.listdir('/proc/self/fd'):
        fd = int(name)
        try:
            mode = os.fstat(fd).st_mode
        except OSError:
            continue  # The directory iterator's descriptor has already closed.
        if fd > 2 and fd not in delegated:
            raise IsolationError('unlisted inherited descriptor')
        if fd <= 2 and stat.S_ISSOCK(mode):
            raise IsolationError('socket inherited as standard stream')
    return frozenset(delegated)
