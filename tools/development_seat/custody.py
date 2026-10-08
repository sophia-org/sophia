"""Pidfd custody and monotonic deadlines; no PID-name or process-group kills."""
import os
import select
import signal
import subprocess
import time
from contextlib import contextmanager


@contextmanager
def defer_term():
    """Keep TERM from interrupting child construction and handle assignment.

    Latch the request instead of blocking the signal: a forked child must not
    inherit a blocked TERM mask. Exec resets this caught disposition normally.
    """
    pending = False
    def remember(*_):
        nonlocal pending
        pending = True
    previous = signal.signal(signal.SIGTERM, remember)
    try:
        yield
    finally:
        signal.signal(signal.SIGTERM, previous)
        if pending:
            signal.raise_signal(signal.SIGTERM)


class Child:
    def __init__(self, argv, **kwargs):
        self.process = subprocess.Popen(argv, close_fds=True, **kwargs)
        try:
            # An unreaped child keeps this PID allocated. Open before any poll
            # or wait, and use only this pidfd to signal it.
            self.pidfd = os.pidfd_open(self.process.pid, 0)
        except BaseException:
            self.process.kill()  # still our unreaped direct child
            self.process.wait()
            raise

    def ready(self, timeout=0):
        if self.pidfd is None:
            return True
        return bool(select.select([self.pidfd], [], [], timeout)[0])

    def send(self, number):
        if self.pidfd is None:
            return
        try:
            signal.pidfd_send_signal(self.pidfd, number)
        except ProcessLookupError:
            pass

    def wait_until(self, deadline):
        while not self.ready(min(0.1, max(0, deadline - time.monotonic()))):
            if time.monotonic() >= deadline:
                raise TimeoutError("development launch deadline expired")
        # Caller can inspect the still-unreaped child's login before finish.

    def finish(self, terminate=False, grace=2):
        if self.pidfd is None:
            return self.process.returncode
        if terminate and not self.ready():
            self.send(signal.SIGTERM)
            if not self.ready(grace):
                self.send(signal.SIGKILL)
        if not self.ready(5):
            raise TimeoutError("owned child did not become reapable")
        result = self.process.wait(timeout=0)
        # Retain custody if a wait is interrupted or fails: the caller's
        # finally block must still be able to terminate and reap this child.
        os.close(self.pidfd)
        self.pidfd = None
        return result


def command(config, argv):
    return [config["tools"]["custody"]["path"], str(os.getpid()), "--", *argv]
