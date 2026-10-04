"""The guest line stamper's diagnostic SysRq option must stay off unless
enabled, fire once only on the first real hard-stall record (as Session's
tracing decorates it), keep that record ahead of its report, report a failed
request by step and errno, and copy the kernel records the request produced.
Fake trigger and kmsg paths stand in for /proc/sysrq-trigger and /dev/kmsg."""
from pathlib import Path
import os
import shutil
import subprocess
import tempfile
import unittest

SOURCE = Path(__file__).resolve().parents[1] / "qemu_line_stamp.c"
# series-10 baseline-3, byte for byte: the hard stall as Session logged it.
STALL = (
    "\x1b[2m2026-10-04T05:59:13.059581Z\x1b[0m \x1b[31mERROR\x1b[0m \x1b[2msophia_backend_live::"
    "production_session::native_scanout::persistent_native_scanout\x1b[0m\x1b[2m:\x1b[0m "
    "sophia_live_native_page_flip_stall schema=3 status=hard_stall output=1 head=1 index=0 "
    "group=0 age_ms=500 generation=1 submissions=90 retirements=88 callbacks=88 "
    "ever_retired=true callback_serial=88 in_flight_ticks=0 submitted_sequence=90 "
    "peer_age_ms=[1:-1] completion_mode=OutFenceAuthoritative completion_fence=Pending "
    "completion_ledger_pending=false out_fence_retirements=87 late_page_flip_events=87 "
    "completion_fence_errors=0 poller_pending=0 poller_routes=1 poller_last_read=WouldBlock "
    "poller_last_decoded=0 poller_last_rejected=0 poller_read_calls=6318 "
    "poller_would_block_reads=6230 poller_read_failures=0 poller_decoded_total=88 "
    "poller_rejected_total=0 poller_emitted_total=88 action=terminate_session")
LOCKED = "sophia_live_session_lock schema=1 status=locked epoch=1"
TRIGGERED = "sophia_qemu_sysrq schema=1 status=triggered key=w"


class StamperTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.directory = tempfile.mkdtemp()
        cls.stamper = os.path.join(cls.directory, "stamp")
        subprocess.run(["cc", "-std=c11", "-O2", "-Wall", "-Wextra", "-Werror",
                        str(SOURCE), "-o", cls.stamper], check=True)

    @classmethod
    def tearDownClass(cls):
        shutil.rmtree(cls.directory)

    def setUp(self):
        self.work = tempfile.mkdtemp()
        self.addCleanup(shutil.rmtree, self.work)
        self.trigger = os.path.join(self.work, "sysrq-trigger")
        Path(self.trigger).write_text("")

    def run_stamper(self, lines, *args, kmsg="/dev/null"):
        result = subprocess.run(
            [self.stamper, *args, f"--sysrq-trigger={self.trigger}", f"--kmsg={kmsg}"],
            input="".join(f"{line}\n" for line in lines), capture_output=True,
            text=True, timeout=30, check=True)
        return result.stdout.splitlines()

    def test_off_unless_enabled(self):
        out = self.run_stamper(["a", STALL, LOCKED])
        self.assertEqual(Path(self.trigger).read_text(), "")
        self.assertNotIn("sophia_qemu_sysrq", "\n".join(out))
        self.assertEqual([line for line in out if not line.startswith("sophia_qemu_stamp ")],
                         ["a", STALL, LOCKED])

    def test_fires_once_on_the_decorated_record_and_keeps_it_first(self):
        out = self.run_stamper(["a", STALL, "b", STALL], "--sysrq-on-hard-stall")
        self.assertEqual(Path(self.trigger).read_text(), "w")
        self.assertEqual(out.count(TRIGGERED), 1)
        self.assertEqual(out.count(STALL), 2)
        self.assertEqual(out.index(TRIGGERED), out.index(STALL) + 1)
        self.assertIn("sophia_qemu_sysrq schema=1 status=captured records=0 lost=0 truncated=0 errno=0", out)

    def test_bare_record_fires_and_near_misses_do_not(self):
        bare = STALL[STALL.index("sophia_live_native_page_flip_stall"):]
        self.run_stamper([bare], "--sysrq-on-hard-stall")
        self.assertEqual(Path(self.trigger).read_text(), "w")
        for miss in (bare.replace("status=hard_stall", "status=soft_stall"),
                     bare.replace("schema=3", "schema=2"),
                     "x" + bare,
                     STALL.replace(" sophia_live_native", " other_sophia_live_native")):
            with self.subTest(miss=miss[:60]):
                Path(self.trigger).write_text("")
                out = self.run_stamper([miss], "--sysrq-on-hard-stall")
                self.assertEqual(Path(self.trigger).read_text(), "")
                self.assertEqual(out, [miss])

    def test_a_failed_request_is_reported_by_step_and_errno(self):
        self.trigger = os.path.join(self.work, "absent", "sysrq-trigger")
        out = self.run_stamper([STALL, LOCKED], "--sysrq-on-hard-stall")
        self.assertEqual(out[0], STALL)
        self.assertEqual(out[1], "sophia_qemu_sysrq schema=1 status=failed step=open errno=2")
        self.assertEqual(out[-1], LOCKED)

    def test_kernel_records_are_copied_without_continuations(self):
        fifo = os.path.join(self.work, "kmsg")
        os.mkfifo(fifo)
        # Held open for writing, so the stamper reads what is queued, then EAGAIN.
        holder = os.open(fifo, os.O_RDWR | os.O_NONBLOCK)
        self.addCleanup(os.close, holder)
        os.write(holder, b"6,101,500,-;sysrq: Show Blocked State\n"
                         b" SUBSYSTEM=tty\n"
                         b"6,102,501,-;task:kworker/u8:3 state:D stack:0 pid:42\n")
        out = self.run_stamper([STALL], "--sysrq-on-hard-stall", kmsg=fifo)
        self.assertEqual(out[1:], [
            TRIGGERED,
            "sophia_qemu_kmsg schema=1 seq=101 text=sysrq: Show Blocked State",
            "sophia_qemu_kmsg schema=1 seq=102 text=task:kworker/u8:3 state:D stack:0 pid:42",
            "sophia_qemu_sysrq schema=1 status=captured records=2 lost=0 truncated=0 errno=0"])

    def test_the_copy_is_bounded_and_says_so(self):
        import fcntl
        fifo = os.path.join(self.work, "kmsg")
        os.mkfifo(fifo)
        holder = os.open(fifo, os.O_RDWR | os.O_NONBLOCK)
        self.addCleanup(os.close, holder)
        fcntl.fcntl(holder, fcntl.F_SETPIPE_SZ, 1 << 20)
        os.write(holder, b"".join(b"6,%d,0,-;record\n" % seq for seq in range(4100)))
        out = self.run_stamper([STALL], "--sysrq-on-hard-stall", kmsg=fifo)
        self.assertEqual(sum(line.startswith("sophia_qemu_kmsg ") for line in out), 4096)
        self.assertEqual(out[-1], "sophia_qemu_sysrq schema=1 status=captured records=4096 lost=0 truncated=1 errno=0")

    def test_unreadable_kmsg_still_triggers_and_reports(self):
        out = self.run_stamper([STALL], "--sysrq-on-hard-stall",
                               kmsg=os.path.join(self.work, "absent"))
        self.assertEqual(out[1:], [TRIGGERED,
                                   "sophia_qemu_sysrq schema=1 status=captured records=0 lost=0 truncated=0 errno=2"])

    def test_unknown_arguments_are_refused(self):
        result = subprocess.run([self.stamper, "--sysrq"], input="", capture_output=True,
                                text=True, timeout=30, check=False)
        self.assertEqual(result.returncode, 2)


if __name__ == "__main__":
    unittest.main()
