"""Supplied builds/VT/native effects; real profile generator and transcript verifier."""
from pathlib import Path
import shutil
import subprocess
import sys

HERE = Path(__file__).resolve()
ROOT = HERE.parents[4]
sys.path.insert(0, str(ROOT / 'tools/probes/lom_workload/tests'))
import test_launcher as panel_tests
from test_native_verify import transcript


class ComponentCommandTests(panel_tests.LauncherTests):
    def setUp(self):
        super().setUp()
        shutil.copytree(ROOT / 'tools/probes/native_launcher', self.tools / 'probes/native_launcher',
                        ignore=shutil.ignore_patterns('__pycache__'))
        shutil.copyfile(ROOT / 'tools/fixtures/native_launcher_core.kdl', self.tools / 'fixtures/native_launcher_core.kdl')
        self.script(self.root / 'target/release/examples/desktop_profile_probe', """
[[ "$#" == 2 || ( "$#" == 3 && "$3" == --require-launcher-binding ) ]]
cat "$1"
tail -n +2 "$2"
""")
        self.bemenu = self.base / 'bemenu'
        self.bemenu.mkdir()
        git = self.fakebin / 'git'
        git.write_text(git.read_text().replace('verify-commit HEAD', 'verify-commit ').replace('    *) exit 99', '    *"archive"*) tar -C "$SOPHIA_BEMENU_SOURCE" -cf - . ;;\n    *) exit 99'))
        self.env['SOPHIA_BEMENU_SOURCE'] = str(self.bemenu)
        self.script(self.fakebin / 'make', '''
echo bemenu-build >> "$TEST_TRACE"
printf '%s\\n' 'fixture Bemenu binary' > "$2/bemenu-sophia"
''')
        # Keep the panel path exactly as the inherited controls require.
        prior = (self.tools / 'run_sophia_session.sh').read_text().splitlines()[2:]
        self.script(self.tools / 'run_sophia_session.sh', '''
if [[ "$#" == 2 ]]; then
    [[ "$1" == --max-runtime-ms=90000 && "$2" == --wm-process="$SOPHIA_HAGIA_BIN" ]]
    [[ "$SOPHIA_SESSION_STARTUP" == none ]]
    [[ "$SOPHIA_REQUIRE_LOCAL_VT" == true && "$SOPHIA_MANAGE_KEYD" == true ]]
    echo component-session >> "$TEST_TRACE"
    mkdir -p "$SOPHIA_DIAGNOSTIC_DIR"
    cp "$TEST_COMPONENT_HOST" "$SOPHIA_DIAGNOSTIC_DIR/events.0.log"
    printf '%s\\n' 'sophia_tty_recovery schema=3 termios_restored=true done=true' \\
      'sophia_tty_recovery_verification schema=1 keyd_restored=true' > "$SOPHIA_DIAGNOSTIC_DIR/recovery.log"
    if [[ "${TEST_CHANGE_BEMENU:-no}" == yes ]]; then
        echo changed >> "$SOPHIA_LOM_NATIVE_EVIDENCE_DIR/bemenu-sophia"
    fi
    exit "${TEST_SESSION_STATUS:-0}"
fi
''' + '\n'.join(prior))
        self.component_host = self.base / 'component-host.log'
        self.component_host.write_text('\n'.join(transcript()) + '\n')
        self.env['TEST_COMPONENT_HOST'] = str(self.component_host)

    def run_components(self, **env):
        return subprocess.run(['bash', str(self.tools / 'run.sh'), 'launcher'],
                              env={**self.env, **env}, capture_output=True, text=True, timeout=10)

    def test_component_mode_uses_roles_and_real_transcript_verifier(self):
        result = self.run_components()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn('bemenu-build', (self.base / 'trace').read_text())
        self.assertIn('component-session', (self.base / 'trace').read_text())
        profile = (self.evidence / 'desktop.kdl').read_text()
        self.assertIn('shell-component "panel" "bar"', profile)
        self.assertIn('shell-component "menu" "application-launcher"', profile)
        self.assertIn('bind "Super+4"', profile)
        self.assertIn('bemenu_commit=', (self.evidence / 'identity.manifest').read_text())
        self.assertIn(str(self.evidence / 'bemenu-sophia'), (self.evidence / 'inputs.sha256').read_text())
        self.assertFalse((self.evidence / 'workload-verification.json').exists())
        self.assertIn('"status": "pass"', (self.evidence / 'launcher-verification.json').read_text())

    def test_component_binary_change_refuses(self):
        self.assertNotEqual(self.run_components(TEST_CHANGE_BEMENU='yes').returncode, 0)
        self.assertFalse((self.evidence / 'launcher-verification.json').exists())

    def test_component_missing_presentation_refuses(self):
        self.component_host.write_text('\n'.join(v for v in transcript() if not ('status=presented' in v and 'connection_epoch=2' in v)))
        self.assertNotEqual(self.run_components().returncode, 0)
        self.assertIn('"status": "fail"', (self.evidence / 'launcher-verification.json').read_text())

    def test_component_failed_preflight_never_starts(self):
        self.assertNotEqual(self.run_components(TEST_PROOF_STATUS='1').returncode, 0)
        self.assertNotIn('component-session', (self.base / 'trace').read_text())
