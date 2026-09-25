#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source "$root/tools/lib/session_profile.sh"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
export SOPHIA_PREFLIGHT_CALLS="$work/calls"
printf 'schema 1\n' >"$work/profile.kdl"
cat >"$work/sophia" <<'STUB'
#!/usr/bin/env bash
printf 'engine:%s\n' "$*" >>"$SOPHIA_PREFLIGHT_CALLS"
[[ "$1" == config && "$2" == check-session-profile ]] || exit 99
if [[ "${SOPHIA_TEST_OLD_BINARY:-0}" != 1 ]]; then
    echo 'sophia_session_profile_preflight schema=1 status=accepted policy=validated'
fi
exit "${SOPHIA_TEST_ENGINE_STATUS:-0}"
STUB
cat >"$work/hagia" <<'STUB'
#!/usr/bin/env bash
printf 'wm:%s\n' "$*" >>"$SOPHIA_PREFLIGHT_CALLS"
exit "${SOPHIA_TEST_WM_STATUS:-0}"
STUB
chmod 700 "$work/sophia" "$work/hagia"
sophia_check_hagia_profile "$work/sophia" "$work/hagia" "$work/profile.kdl"
[[ "$(wc -l <"$work/calls")" == 1 ]]
# Role selection, policy rejection, private staging and timeout are exercised
# against the real binary by the session_profile_preflight Rust tests.
if SOPHIA_TEST_OLD_BINARY=1 sophia_check_hagia_profile "$work/sophia" "$work/hagia" "$work/profile.kdl"; then
    echo 'An unsupported preflight operation returned zero and was accepted' >&2; exit 1
fi
: >"$work/calls"
if SOPHIA_TEST_ENGINE_STATUS=1 sophia_check_hagia_profile "$work/sophia" "$work/hagia" "$work/profile.kdl"; then
    echo 'Engine rejection was ignored' >&2; exit 1
fi
[[ "$(wc -l <"$work/calls")" == 1 ]]
# Run the real TTY adapter on a disposable PTY. Refused policy must stop before
# the first TTY-mode query or privileged handoff. Neither can touch the host.
python3 - "$root" "$work" <<'PY'
import os, pathlib, pty, shlex, shutil, subprocess, sys
root, work = map(pathlib.Path, sys.argv[1:])
tree = work/'tree'
(tree/'tools/lib').mkdir(parents=True)
for rel in ('tools/start_sophia_tty3.sh', 'tools/lib/session_profile.sh'):
    shutil.copy2(root/rel, tree/rel)
launcher = tree/'tools/start_sophia_tty3.sh'
launcher.write_text(launcher.read_text().replace(
    'LAUNCH_LOG="/tmp/sophia-${SESSION_PROFILE}-tty${TARGET_VT}-launch.log"',
    'LAUNCH_LOG='+shlex.quote(str(work/'shared-launcher.log'))))
marker = work/'takeover'
for name, script in {
    'tty': '#!/bin/sh\necho /dev/tty3\n',
    'sudo': '#!/bin/sh\ntouch "$SOPHIA_TAKEOVER_MARKER"\nexit 97\n',
    'python3': '#!/bin/sh\ntouch "$SOPHIA_TAKEOVER_MARKER"\nexit 97\n',
}.items():
    p=work/name; p.write_text(script); p.chmod(0o700)
env=dict(os.environ, PATH=str(work)+':'+os.environ['PATH'],
    SOPHIA_TTY_PROFILE='hagia', SOPHIA_TTY_NUMBER='3', SOPHIA_BUILD_SESSION='false',
    SOPHIA_BIN=str(work/'sophia'), SOPHIA_HAGIA_BIN=str(work/'hagia'),
    SOPHIA_DESKTOP_PROFILE=str(work/'profile.kdl'), SOPHIA_TEST_ENGINE_STATUS='1',
    XDG_STATE_HOME=str(work/'state'), SOPHIA_TAKEOVER_MARKER=str(marker))
master, slave=pty.openpty()
try:
    with (work/'launcher.log').open('w') as log:
        result=subprocess.run(['bash',str(tree/'tools/start_sophia_tty3.sh')],
            stdin=slave,stdout=log,stderr=log,env=env,timeout=15)
    assert result.returncode == 1, (work/'launcher.log').read_text()
    assert not marker.exists(), 'Handoff began before WM validation passed'
    assert (work/'calls').read_text().splitlines()[-1].startswith('engine:config check-session-profile')
finally:
    os.close(slave); os.close(master)
PY
echo 'Hagia profile preflight checks passed'
