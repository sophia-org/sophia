"""CPU custody controls inside the existing isolated gate's device-free namespace."""
import importlib.util
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch


SOURCE = Path(__file__).resolve().parents[1] / "development_seat"
spec = importlib.util.spec_from_file_location("development_custody", SOURCE / "custody.py")
custody = importlib.util.module_from_spec(spec)
spec.loader.exec_module(custody)

PAUSE_BEFORE_ARM = r'''
#define _GNU_SOURCE
#include <errno.h>
#include <stdlib.h>
#include <sys/prctl.h>
#include <sys/syscall.h>
#include <unistd.h>
int prctl(int option, ...) {
    if (option != PR_SET_PDEATHSIG || getpid() != 1) { errno=EINVAL; return -1; }
    int ready=atoi(getenv("FIXTURE_READY_FD"));
    int release=atoi(getenv("FIXTURE_RELEASE_FD"));
    char byte='R';
    if (write(ready,&byte,1)!=1 || read(release,&byte,1)!=1) _exit(126);
    close(ready); close(release);
    return syscall(SYS_prctl,PR_SET_PDEATHSIG,9L,0L,0L,0L);
}
'''


class Custody(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temporary = tempfile.TemporaryDirectory(prefix="development-custody-")
        cls.addClassCleanup(cls.temporary.cleanup)
        cls.directory = Path(cls.temporary.name)
        for name, source in (("custody-exec", "custody_exec.c"), ("guard", "namespace_guard.c")):
            subprocess.run(["cc", "-std=c11", "-O2", "-Wall", "-Wextra", "-Werror",
                            str(SOURCE / source), "-o", str(cls.directory / name)],
                           check=True, capture_output=True, timeout=30)
        shim = cls.directory / "pause.c"
        shim.write_text(PAUSE_BEFORE_ARM)
        subprocess.run(["cc", "-shared", "-fPIC", "-Wall", "-Wextra", "-Werror", str(shim),
                        "-o", str(cls.directory / "pause.so")],
                       check=True, capture_output=True, timeout=30)

    def child(self, code):
        return custody.Child([str(self.directory / "custody-exec"), str(os.getpid()), "--",
                              sys.executable, "-I", "-B", "-c", code],
                             stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                             stderr=subprocess.DEVNULL)

    def test_normal_exit_is_reaped_with_the_original_status(self):
        child = self.child("raise SystemExit(3)")
        child.wait_until(time.monotonic() + 5)
        self.assertEqual(child.finish(), 3)
        self.assertIsNone(child.pidfd)
        self.assertEqual(child.finish(terminate=True), 3)

    def test_failed_wait_retains_custody_for_finally_cleanup(self):
        child = self.child("import time; time.sleep(30)")
        descriptor = child.pidfd
        try:
            with patch.object(child, "ready", side_effect=InterruptedError("fixture wait")):
                with self.assertRaisesRegex(InterruptedError, "fixture wait"):
                    child.finish()
            self.assertEqual(child.pidfd, descriptor, "failed wait keeps the pidfd")
            os.fstat(descriptor)
        finally:
            self.assertEqual(child.finish(terminate=True), -signal.SIGTERM)

    def test_deadline_kills_a_term_ignoring_owned_child(self):
        read_fd, write_fd = os.pipe()
        code = f"import os,signal,time; signal.signal(signal.SIGTERM,signal.SIG_IGN); os.write({write_fd},b'ready'); os.close({write_fd}); time.sleep(30)"
        child = custody.Child([str(self.directory / "custody-exec"), str(os.getpid()), "--",
                               sys.executable, "-I", "-B", "-c", code],
                              stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                              stderr=subprocess.PIPE, pass_fds=(write_fd,))
        os.close(write_fd)
        try:
            self.assertEqual(os.read(read_fd, 5), b"ready")
            with self.assertRaises(TimeoutError):
                child.wait_until(time.monotonic() + 0.03)
            self.assertEqual(child.finish(terminate=True), -signal.SIGKILL)
        finally:
            os.close(read_fd)
            child.process.stderr.close()

    def test_a_lost_parent_refuses_before_exec(self):
        marker = self.directory / "must-not-execute"
        result = subprocess.run([str(self.directory / "custody-exec"), str(os.getpid() + 1000000), "--",
                                 sys.executable, "-I", "-B", "-c",
                                 f"from pathlib import Path; Path({str(marker)!r}).touch()"],
                                stdin=subprocess.DEVNULL, capture_output=True, timeout=5)
        self.assertEqual(result.returncode, 2)
        self.assertFalse(marker.exists())

    def test_namespace_guard_keeps_status_and_kills_descendants_with_its_monitor(self):
        # This root is only UID 0 in a new unprivileged user namespace. Devices
        # are replaced before the fixture executes. No PAM/logind/host-root use.
        import shutil
        bwrap = shutil.which("bwrap")
        script = r'''
import json, os, pathlib, select, signal, subprocess, sys, time
guard=sys.argv[1]
plain=subprocess.run([guard,'--','/usr/bin/false'],timeout=5)
assert plain.returncode == 1
code="""
import os,pathlib,time
pid=os.fork()
if pid == 0:
    while True: time.sleep(1)
outer_pid=int(pathlib.Path('/proc/self/stat').read_text().split(' ',1)[0])
children=pathlib.Path('/proc/self/task/'+str(outer_pid)+'/children').read_text().split()
print(str(outer_pid)+' '+children[0],flush=True)
while True: time.sleep(1)
"""
monitor=subprocess.Popen([guard,'--',sys.executable,'-I','-B','-c',code],stdout=subprocess.PIPE,text=True)
fd=os.pidfd_open(monitor.pid)
assert select.select([monitor.stdout],[],[],5)[0], 'namespace child did not start'
pids=list(map(int,monitor.stdout.readline().split()))
assert len(pids)==2
signal.pidfd_send_signal(fd,signal.SIGKILL)
assert monitor.wait(timeout=5)==-signal.SIGKILL
os.close(fd)
deadline=time.monotonic()+5
while time.monotonic()<deadline:
    active=[]
    for pid in pids:
        try: state=pathlib.Path('/proc/'+str(pid)+'/stat').read_text().rsplit(') ',1)[1].split()[0]
        except FileNotFoundError: continue
        if state not in ('Z','X'): active.append(pid)
    if not active: break
    time.sleep(.01)
assert not active, ('namespace descendants survived', active)
print('namespace guard descendants gone')
'''
        result = subprocess.run([bwrap, "--unshare-all", "--uid", "0", "--gid", "0",
                                 "--cap-add", "CAP_SYS_ADMIN", "--die-with-parent",
                                 "--ro-bind", "/", "/", "--dev", "/dev", "--proc", "/proc",
                                 "--", sys.executable, "-I", "-B", "-c", script,
                                 str(self.directory / "guard")],
                                stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=20)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("descendants gone", result.stdout)

    def test_parent_death_before_namespace_init_arms_refuses_before_exec(self):
        import shutil
        # The test-only preload pauses the *real guard* just before its first
        # PR_SET_PDEATHSIG. Kill the monitor, then release that call: only its
        # pre-clone pidfd check can close this missed-parent-death window.
        script = r'''
import os,pathlib,select,signal,subprocess,sys,time
guard,shim,marker=sys.argv[1:]
rr,rw=os.pipe(); gr,gw=os.pipe()
environment=dict(os.environ,LD_PRELOAD=shim,FIXTURE_READY_FD=str(rw),FIXTURE_RELEASE_FD=str(gr))
process=subprocess.Popen([guard,'--','/usr/bin/touch',marker],env=environment,pass_fds=(rw,gr))
fd=os.pidfd_open(process.pid)
os.close(rw); os.close(gr)
assert select.select([rr],[],[],5)[0], 'child did not reach pre-arm barrier'
assert os.read(rr,1)==b'R'
children=pathlib.Path('/proc/'+str(process.pid)+'/task/'+str(process.pid)+'/children').read_text().split()
assert len(children)==1
child=int(children[0])
signal.pidfd_send_signal(fd,signal.SIGKILL)
assert process.wait(timeout=5)==-signal.SIGKILL
os.close(fd)
os.write(gw,b'G'); os.close(gw); os.close(rr)
deadline=time.monotonic()+5
while time.monotonic()<deadline:
    try: state=pathlib.Path('/proc/'+str(child)+'/stat').read_text().rsplit(') ',1)[1].split()[0]
    except FileNotFoundError: break
    if state in ('Z','X'): break
    time.sleep(.01)
else: raise AssertionError('orphan namespace init stayed alive')
assert not pathlib.Path(marker).exists(), 'missed parent death reached exec'
print('pre-arm parent death refused')
'''
        marker = self.directory / "race-must-not-execute"
        result = subprocess.run([shutil.which("bwrap"), "--unshare-all", "--uid", "0", "--gid", "0",
                                 "--cap-add", "CAP_SYS_ADMIN", "--die-with-parent", "--ro-bind", "/", "/",
                                 "--bind", str(self.directory), str(self.directory),
                                 "--dev", "/dev", "--proc", "/proc", "--", sys.executable,
                                 "-I", "-B", "-c", script, str(self.directory / "guard"),
                                 str(self.directory / "pause.so"), str(marker)],
                                stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=20)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("pre-arm parent death refused", result.stdout)


if __name__ == "__main__":
    unittest.main()
