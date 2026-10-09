import os
from pathlib import Path
import shlex
import socket
import subprocess
import tempfile
import threading
import time
import unittest


ROOT = Path(__file__).resolve().parents[2]
VALID = ["--list-only", "--geometry", "0,0,16,16", "--format", "XR24"]


class Dri3LayoutProbeTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.directory = tempfile.TemporaryDirectory(prefix="sophia-layout-cli-")
        cls.binary = Path(cls.directory.name) / "probe"
        flags = subprocess.check_output([
            "pkg-config", "--cflags", "--libs", "xcb", "xcb-dri3", "xcb-present", "gbm"
        ], text=True, timeout=10)
        subprocess.run([
            "cc", "-std=c11", "-O2", "-Wall", "-Wextra", "-Werror",
            str(ROOT / "tools/probes/dri3_layout.c"), "-o", str(cls.binary),
            *shlex.split(flags),
        ], check=True, capture_output=True, timeout=30)

    @classmethod
    def tearDownClass(cls):
        cls.directory.cleanup()

    def run_probe(self, arguments, **environment):
        env = os.environ.copy()
        for name in ("DISPLAY", "WAYLAND_DISPLAY", "LD_PRELOAD"):
            env.pop(name, None)
        env["XAUTHORITY"] = "/dev/null"
        env.update(environment)
        return subprocess.run(
            [str(self.binary), *arguments], env=env, capture_output=True,
            text=True, timeout=3, check=False,
        )

    def test_help_and_invalid_arguments_do_not_connect(self):
        help_result = self.run_probe(["--help"])
        self.assertEqual(help_result.returncode, 0, help_result.stderr)
        self.assertIn("--list-only", help_result.stdout)
        self.assertIn("--managed", help_result.stdout)
        self.assertIn("--report-keys", help_result.stdout)
        invalid = [
            [], ["--list-only"],
            ["--geometry", "0,0,16,16", "--format", "XR24"],
            [*VALID, "--modifier", "0x00ffffffffffffff"],
            [*VALID, "--modifier", "0xffffffffffffffff"],
            [*VALID, "--frames", "0"], [*VALID, "--frames", "121"],
            [*VALID, "--timeout-ms", "0"], [*VALID, "--timeout-ms", "10001"],
            [*VALID, "--unknown", "1"], [*VALID, "--modifier"],
        ]
        for geometry in (
            "0,0,0,16", "0,0,4097,16", "32768,0,16,16", "0,-32769,16,16",
            "0,0,16,16extra", "999999999999999999999999,0,16,16",
        ):
            invalid.append(["--list-only", "--geometry", geometry, "--format", "XR24"])
        for arguments in invalid:
            with self.subTest(arguments=arguments):
                result = self.run_probe(arguments)
                self.assertEqual(result.returncode, 2, result.stderr)
                self.assertIn("usage:", result.stderr)
                self.assertNotIn("stage=authenticated_connect", result.stderr)
                self.assertNotIn("event=finished", result.stdout)

    def test_valid_request_requires_explicit_display(self):
        for arguments in (VALID, [*VALID, "--suboptimal"], [*VALID, "--managed"], [*VALID, "--report-keys"]):
            result = self.run_probe(arguments)
            self.assertEqual(result.returncode, 2, result.stderr)
            self.assertIn("stage=display_unset", result.stderr)
            self.assertNotIn("event=finished", result.stdout)

    def test_setup_wait_is_inside_the_process_deadline(self):
        # This endpoint accepts only this child and never answers X setup.
        # Empty Xauthority prevents reading or forwarding any session cookie.
        with socket.socket() as listener:
            listener.bind(("127.0.0.1", 0))
            listener.listen(1)
            listener.settimeout(2)
            port = listener.getsockname()[1]
            self.assertGreaterEqual(port, 6000)
            accepted = threading.Event()
            release = threading.Event()
            failures = []

            def serve():
                try:
                    with listener.accept()[0] as peer:
                        peer.settimeout(2)
                        if peer.recv(4096):
                            accepted.set()
                        release.wait(2)
                except OSError as error:
                    failures.append(type(error).__name__)

            worker = threading.Thread(target=serve)
            worker.start()
            started = time.monotonic()
            try:
                result = self.run_probe(
                    [*VALID, "--timeout-ms", "100"],
                    DISPLAY=f"127.0.0.1:{port - 6000}",
                )
                elapsed = time.monotonic() - started
            finally:
                release.set()
                worker.join(3)
            self.assertFalse(worker.is_alive(), "private endpoint did not terminate")
            self.assertFalse(failures, failures)
            self.assertTrue(accepted.is_set(), "child never attempted X setup")
            self.assertEqual(result.returncode, 124, result.stderr)
            self.assertIn("stage=process_deadline", result.stderr)
            self.assertNotIn("event=finished", result.stdout)
            self.assertLess(elapsed, 2, "setup escaped the watchdog")

    def test_only_a_managed_window_leaves_its_position_to_the_window_manager(self):
        # The probe's own decisions, compiled from its source: which window
        # attributes it creates with, and which geometry it accepts after the
        # map and in ConfigureNotify.
        driver = Path(self.directory.name) / "decisions.c"
        driver.write_text(
            '#define main dri3_layout_main\n'
            f'#include "{ROOT / "tools/probes/dri3_layout.c"}"\n'
            '#undef main\n'
            'static void attributes(const char *mode, bool managed)\n'
            '{\n'
            '    struct options options = { .managed = managed };\n'
            '    uint32_t values[4] = { 0 };\n'
            '    uint32_t mask = window_attributes(&options, 77, values);\n'
            '    printf("%s override_redirect=%d values=%u,%u,%u,%u\\n", mode,\n'
            '        (mask & XCB_CW_OVERRIDE_REDIRECT) != 0, values[0], values[1], values[2], values[3]);\n'
            '}\n'
            'static void parsed(const char *mode, int argc, char **argv)\n'
            '{\n'
            '    struct options options;\n'
            '    bool valid = parse(argc, argv, &options);\n'
            '    printf("%s parsed=%d managed=%d\\n", mode, valid, options.managed);\n'
            '}\n'
            'int main(void)\n'
            '{\n'
            '    char *strict[] = { "probe", "--list-only", "--geometry", "0,0,16,16", "--format", "XR24" };\n'
            '    char *managed[] = { "probe", "--list-only", "--geometry", "0,0,16,16", "--format", "XR24",\n'
            '                        "--managed" };\n'
            '    parsed("strict", 6, strict);\n'
            '    parsed("managed", 7, managed);\n'
            '    attributes("strict", false);\n'
            '    attributes("managed", true);\n'
            '    static const int cases[][4] = {\n'
            '        { 100, 100, 400, 300 }, { 0, 100, 400, 300 }, { 100, 0, 400, 300 },\n'
            '        { 1280, 0, 400, 300 }, { 100, 100, 401, 300 }, { 100, 100, 400, 299 },\n'
            '        { 0, 0, 1280, 800 },\n'
            '    };\n'
            '    for (int managed = 0; managed < 2; ++managed) {\n'
            '        struct options options = { .x = 100, .y = 100, .width = 400, .height = 300,\n'
            '                                   .managed = managed };\n'
            '        for (unsigned i = 0; i < sizeof cases / sizeof cases[0]; ++i)\n'
            '            printf("%s %d,%d,%d,%d=%d\\n", managed ? "managed" : "strict",\n'
            '                cases[i][0], cases[i][1], cases[i][2], cases[i][3],\n'
            '                geometry_matches(&options, cases[i][0], cases[i][1], cases[i][2], cases[i][3]));\n'
            '    }\n'
            '    return 0;\n'
            '}\n'
        )
        binary = Path(self.directory.name) / "decisions"
        flags = subprocess.check_output([
            "pkg-config", "--cflags", "--libs", "xcb", "xcb-dri3", "xcb-present", "gbm"
        ], text=True, timeout=10)
        subprocess.run([
            "cc", "-std=c11", "-O2", "-Wall", "-Wextra", "-Werror", "-Wno-unused-function",
            str(driver), "-o", str(binary), *shlex.split(flags),
        ], check=True, capture_output=True, timeout=30)
        lines = subprocess.run(
            [str(binary)], capture_output=True, text=True, timeout=3, check=True,
        ).stdout.splitlines()
        structure = 1 << 17  # XCB_EVENT_MASK_STRUCTURE_NOTIFY
        self.assertEqual(lines[:2], [
            "strict parsed=1 managed=0", "managed parsed=1 managed=1",
        ])
        lines = lines[2:]
        self.assertEqual(lines[:2], [
            f"strict override_redirect=1 values={0xff304038},1,{structure},77",
            f"managed override_redirect=0 values={0xff304038},{structure},77,0",
        ])
        self.assertEqual(lines[2:], [
            # Strict: any drift, of position or size, is refused.
            "strict 100,100,400,300=1", "strict 0,100,400,300=0",
            "strict 100,0,400,300=0", "strict 1280,0,400,300=0",
            "strict 100,100,401,300=0", "strict 100,100,400,299=0",
            "strict 0,0,1280,800=0",
            # Managed: the WM may place it anywhere, never resize it.
            "managed 100,100,400,300=1", "managed 0,100,400,300=1",
            "managed 100,0,400,300=1", "managed 1280,0,400,300=1",
            "managed 100,100,401,300=0", "managed 100,100,400,299=0",
            "managed 0,0,1280,800=0",
        ])


    def test_report_keys_selects_and_reports_only_focus_and_key_presses(self):
        # The probe's own --report-keys decisions, compiled from its source: the
        # parsed switch, the event mask it creates the window with, and the
        # lines report_input writes for focus changes and key presses.
        driver = Path(self.directory.name) / "report_keys.c"
        driver.write_text(
            '#define main dri3_layout_main\n'
            f'#include "{ROOT / "tools/probes/dri3_layout.c"}"\n'
            '#undef main\n'
            'static void focus(struct probe *probe, uint8_t type, uint8_t mode, uint8_t detail)\n'
            '{\n'
            '    xcb_focus_in_event_t event = { .response_type = type, .mode = mode, .detail = detail };\n'
            '    report_input(probe, (xcb_generic_event_t *)&event);\n'
            '}\n'
            'static void key(struct probe *probe, uint8_t type, uint8_t keycode)\n'
            '{\n'
            '    xcb_key_press_event_t event = { .response_type = type, .detail = keycode };\n'
            '    report_input(probe, (xcb_generic_event_t *)&event);\n'
            '}\n'
            'int main(void)\n'
            '{\n'
            '    char *plain[] = { "probe", "--list-only", "--geometry", "0,0,16,16", "--format", "XR24" };\n'
            '    char *keys[] = { "probe", "--list-only", "--geometry", "0,0,16,16", "--format", "XR24",\n'
            '                     "--managed", "--report-keys" };\n'
            '    struct options options;\n'
            '    bool valid = parse(6, plain, &options);\n'
            '    printf("plain parsed=%d report_keys=%d\\n", valid, options.report_keys);\n'
            '    valid = parse(8, keys, &options);\n'
            '    printf("keys parsed=%d report_keys=%d managed=%d\\n", valid, options.report_keys, options.managed);\n'
            '    uint32_t values[4] = { 0 };\n'
            '    (void)window_attributes(&options, 77, values);\n'
            '    printf("event_mask=%u\\n", values[1]);\n'
            '    fflush(stdout);\n'
            '    struct probe probe = { .options = options };\n'
            '    focus(&probe, XCB_FOCUS_IN, 0, 3);\n'
            '    key(&probe, XCB_KEY_RELEASE, 38);\n'
            '    key(&probe, XCB_KEY_PRESS | 0x80, 38);\n'
            '    focus(&probe, XCB_FOCUS_OUT | 0x80, 1, 4);\n'
            '    for (int i = 0; i < 65; ++i)\n'
            '        key(&probe, XCB_KEY_PRESS, 56);\n'
            '    return 0;\n'
            '}\n'
        )
        binary = Path(self.directory.name) / "report_keys"
        flags = subprocess.check_output([
            "pkg-config", "--cflags", "--libs", "xcb", "xcb-dri3", "xcb-present", "gbm"
        ], text=True, timeout=10)
        subprocess.run([
            "cc", "-std=c11", "-O2", "-Wall", "-Wextra", "-Werror", "-Wno-unused-function",
            str(driver), "-o", str(binary), *shlex.split(flags),
        ], check=True, capture_output=True, timeout=30)
        result = subprocess.run([str(binary)], capture_output=True, text=True, timeout=3, check=True)
        key_press, structure, focus_change = 1 << 0, 1 << 17, 1 << 21
        self.assertEqual(result.stdout.splitlines(), [
            "plain parsed=1 report_keys=0",
            "keys parsed=1 report_keys=1 managed=1",
            f"event_mask={structure | key_press | focus_change}",
        ])
        # A key release is not reported; a sent event (high bit) is reported
        # by its type and named synthetic, a real one not; the 65th press is
        # named as the one overflow.
        self.assertEqual(result.stderr.splitlines(), [
            "dri3_layout stage=focus state=in source=event mode=0 detail=3 synthetic=0",
            "dri3_layout stage=key keycode=38 synthetic=1",
            "dri3_layout stage=focus state=out source=event mode=1 detail=4 synthetic=1",
            *["dri3_layout stage=key keycode=56 synthetic=0"] * 63,
            "dri3_layout stage=key_overflow reported=64",
        ])

if __name__ == "__main__":
    unittest.main()
