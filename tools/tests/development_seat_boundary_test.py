"""CPU controls for the development-seat confinement boundary. No PAM or GPU.

The test entry executes the same scope/filter functions as sandbox-exec, under
the current unprivileged UID. It cannot qualify the root credential transition.
That transition needs a separate privileged fixture before launcher activation.
"""
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
SOURCE = ROOT / "tools/development_seat"
ENTRY = r"""
#define _GNU_SOURCE
#include "restrict.h"
#include <stdio.h>
#include <sys/prctl.h>
#include <unistd.h>
int main(int argc, char **argv) {
    if (argc < 2 || getuid() == 0) return 2;
    if (prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) ||
        development_scope() || development_socket_filter()) {
        perror("boundary entry refused"); return 2;
    }
    execv(argv[1], argv + 1);
    perror("boundary entry exec"); return 2;
}
"""

PROBE = r"""
import ctypes, errno, os, socket, sys, threading
libc = ctypes.CDLL(None, use_errno=True)

def refused(call):
    try:
        value = call()
    except OSError as e:
        assert e.errno == errno.EPERM, e
    else:
        if isinstance(value, socket.socket): value.close()
        raise AssertionError('prohibited operation succeeded')

for family in (socket.AF_INET, socket.AF_INET6, socket.AF_PACKET):
    refused(lambda: socket.socket(family, socket.SOCK_DGRAM))
refused(lambda: socket.socket(socket.AF_NETLINK, socket.SOCK_RAW, 0))
refused(lambda: socket.socketpair(socket.AF_INET, socket.SOCK_STREAM))
uevent = socket.socket(socket.AF_NETLINK, socket.SOCK_DGRAM, 15)
uevent.close()
a, b = socket.socketpair()
a.sendall(b'private'); assert b.recv(7) == b'private'
a.close(); b.close()

# Parent-created abstract endpoint is outside the scope; an endpoint created
# by this domain is reachable. No connection to the desktop or system bus.
outside = socket.socket(socket.AF_UNIX)
refused(lambda: outside.connect('\0' + sys.argv[1]))
outside.close()
inside = socket.socket(socket.AF_UNIX)
inside.bind('\0sophia-boundary-inside-' + str(os.getpid()))
inside.listen(1)
client = socket.socket(socket.AF_UNIX)
client.connect(inside.getsockname())
server, _ = inside.accept()
client.sendall(b'ok'); assert server.recv(2) == b'ok'
client.close(); server.close(); inside.close()

# Signal 0 checks authority without delivering a signal to the parent.
refused(lambda: os.kill(int(sys.argv[2]), 0))
os.kill(os.getpid(), 0)
thread = threading.Thread(target=lambda: None)
thread.start(); thread.join()
print('boundary sockets, signals and thread creation pass')
"""


class DevelopmentBoundary(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temporary = tempfile.TemporaryDirectory(prefix="development-seat-boundary-")
        cls.addClassCleanup(cls.temporary.cleanup)
        cls.directory = Path(cls.temporary.name)
        entry = cls.directory / "entry.c"
        entry.write_text(ENTRY)
        cls.entry = cls.directory / "scope-exec"
        cls.drop = cls.directory / "sandbox-exec"
        for output, main in [(cls.entry, entry), (cls.drop, SOURCE / "sandbox_exec.c")]:
            subprocess.run(["cc", "-std=c11", "-O2", "-Wall", "-Wextra", "-Werror",
                            "-I", str(SOURCE), str(main), str(SOURCE / "restrict.c"),
                            str(SOURCE / "drop.c"), "-o", str(output)], check=True,
                           capture_output=True, timeout=30)

    def run_boundary(self, code, *args):
        return subprocess.run([str(self.entry), sys.executable, "-I", "-B", "-c", code, *args],
                              stdin=subprocess.DEVNULL, capture_output=True, text=True,
                              timeout=20, env={"PATH": "/usr/bin:/bin", "LANG": "C.UTF-8"})

    def test_real_kernel_scopes_refuse_external_ipc_and_keep_internal_ipc(self):
        with socket.socket(socket.AF_UNIX) as listener:
            name = "sophia-boundary-outside-" + str(os.getpid())
            listener.bind("\0" + name)
            listener.listen(1)
            result = self.run_boundary(PROBE, name, str(os.getpid()))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("sockets, signals and thread creation pass", result.stdout)

    def test_native_filter_blocks_syscall_bypasses(self):
        # Linux x86_64 numbers. Other architectures must supply their own
        # syscall-number control; the production filter uses SYS_* constants.
        if os.uname().machine != "x86_64":
            self.skipTest("x86_64 syscall-number control")
        code = r'''
import ctypes, errno
libc=ctypes.CDLL(None, use_errno=True)
for nr in (425, 426, 427, 308, 0x40000027):
    ctypes.set_errno(0)
    assert libc.syscall(nr, -1, 0, 0, 0, 0, 0) == -1, nr
    assert ctypes.get_errno() == errno.EPERM, (nr, ctypes.get_errno())
ctypes.set_errno(0)
assert libc.syscall(435, 0, 0) == -1
assert ctypes.get_errno() == errno.ENOSYS
'''
        result = self.run_boundary(code)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_root_helper_refuses_unprivileged_exec_and_invalid_identity(self):
        for args in ([str(os.getuid()), str(os.getgid()), "--", "/usr/bin/true"],
                     ["0", "1000", "--", "/usr/bin/true"],
                     ["-1", "1000", "--", "/usr/bin/true"],
                     ["1000", "1000", "--", "relative"]):
            with self.subTest(args=args):
                result = subprocess.run([str(self.drop), *args], stdin=subprocess.DEVNULL,
                                        capture_output=True, timeout=5)
                self.assertEqual(result.returncode, 2)

    def test_real_nested_bubblewrap_preserves_the_scope(self):
        # The shape matches ordinary ProtectionDomainSpec namespace creation:
        # no supplied userns/pidns fd, so no setns is required. Runtime's own
        # protected-client test is also required separately before acceptance.
        bwrap = os.environ.get("DEVELOPMENT_SEAT_BWRAP")
        if not bwrap:
            self.skipTest("private bubblewrap build required for bundle qualification")
        self.assertTrue(Path(bwrap).is_absolute())
        marker = self.directory / "nested-marker"
        argv = [bwrap, "--unshare-all", "--unshare-user", "--die-with-parent",
                "--disable-userns", "--assert-userns-disabled", "--as-pid-1",
                "--new-session", "--cap-drop", "ALL", "--ro-bind", "/", "/",
                "--bind", str(self.directory), str(self.directory),
                "--dev", "/dev", "--proc", "/proc", "--", sys.executable,
                "-I", "-B", "-c", r'''
import errno, fcntl, os, pathlib, socket, struct, sys
assert os.readlink('/proc/self/ns/net') != sys.argv[2], 'child net namespace must be private'
interfaces = [line.split(':')[0].strip() for line in pathlib.Path('/proc/net/dev').read_text().splitlines()[2:]]
assert interfaces == ['lo'], interfaces
with socket.socket(socket.AF_UNIX) as query:
    flags = struct.unpack_from('H', fcntl.ioctl(query, 0x8913, struct.pack('16sH', b'lo', 0)), 16)[0]
assert flags & 1 == 0, 'private loopback must stay down'
for family in (socket.AF_INET, socket.AF_NETLINK):
    try: socket.socket(family, socket.SOCK_DGRAM, 0)
    except OSError as error: assert error.errno == errno.EPERM, error
    else: raise AssertionError('nested child bypassed parent filter')
assert not pathlib.Path('/dev/dri').exists()
pathlib.Path(sys.argv[1]).write_text('nested')
''', str(marker), os.readlink('/proc/self/ns/net')]
        result = subprocess.run([str(self.entry), *argv], stdin=subprocess.DEVNULL,
                                capture_output=True, text=True, timeout=20)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(marker.read_text(), "nested")


if __name__ == "__main__":
    unittest.main()
