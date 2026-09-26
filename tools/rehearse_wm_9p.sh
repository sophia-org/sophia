#!/usr/bin/env bash
# Scripted t250 recovery rehearsal. Runs the installed personal release's exact
# binaries, with its sealed Hagia over the selected WM wire, and drives the
# recovery operations through control revision 2 while the session runs:
# restart-wm, reload-profile (unchanged, rejected with rollback, restored) and
# logout. Every phase's outcome is recorded in a fresh evidence directory.
#
# The profile is a writable copy of the release's own, so the rejection case can
# swap in a candidate Hagia refuses (view-count 10) and restore the original.
# Everything else, including every binary, is the sealed release. Run it after
# logging out of the desktop and logging in on tty4. Ctrl+Alt+Backspace remains
# the emergency exit.
set -euo pipefail

usage() {
    echo "usage: tools/rehearse_wm_9p.sh [--wire=9p2000.L|current-ipc] [--release=/abs/dir] [--dry-run]" >&2
    exit 2
}

wire=9p2000.L
release=/opt/sophia-niltempus-desktop/current
dry_run=false
for argument in "$@"; do
    case "$argument" in
        --wire=9p2000.L | --wire=current-ipc) wire=${argument#--wire=} ;;
        --release=/*) release=${argument#--release=} ;;
        --dry-run) dry_run=true ;;
        *) usage ;;
    esac
done
release=$(readlink -f -- "$release")

if [[ "$dry_run" != true ]] && ! [[ -t 0 && "$(tty)" == /dev/tty4 ]]; then
    echo "Log out of the desktop, log in on tty4, then run this again." >&2
    exit 1
fi
# The release must still be exactly what it sealed, binaries and profile alike.
if ! (cd -- "$release" && sha256sum --quiet --check SHA256SUMS); then
    echo "The release no longer matches its SHA256SUMS; refusing to rehearse it." >&2
    exit 1
fi
sophia="$release/target/release/sophia"
hagia="$release/target/release/hagia"
profile="$release/share/sophia-niltempus-desktop/desktop.kdl"
for file in "$sophia" "$hagia" "$release/bin/sophia-hagia-session"; do
    [[ -x "$file" ]] || { echo "release is missing $file" >&2; exit 1; }
done
if ! grep -Eq '^[[:space:]]*view-count [0-9]+[[:space:]]*$' "$profile"; then
    echo "the release profile has no policy view-count to vary" >&2
    exit 1
fi

stamp=$(date -u +%Y%m%dT%H%M%SZ)
evidence="${XDG_STATE_HOME:-$HOME/.local/state}/sophia/development-evidence/t250-rehearsal-$(basename -- "$release")-$wire-$stamp"
mkdir -p -- "$evidence"
chmod 700 -- "$evidence"
cp -- "$profile" "$evidence/desktop-valid.kdl"
cp -- "$profile" "$evidence/desktop.kdl"
sed -E 's/^([[:space:]]*view-count )[0-9]+([[:space:]]*)$/\110\2/' \
    "$evidence/desktop-valid.kdl" >"$evidence/desktop-rejected.kdl"
cp -- "$release/desktop-manifest.json" "$evidence/" 2>/dev/null || true
(cd -- "$release" && sha256sum target/release/sophia target/release/hagia) >"$evidence/binaries.sha256"
printf 'release=%s\nwire=%s\nstarted=%s\n' "$release" "$wire" "$stamp" >"$evidence/candidate.txt"

if [[ "$dry_run" == true ]]; then
    echo "$evidence"
    exit 0
fi

phases="$evidence/phases.ndjson"
record() {
    printf '{"phase":"%s","expected":"%s","observed":"%s","pass":%s}\n' \
        "$1" "$2" "$3" "$4" >>"$phases"
}

# One control invocation with its own bound; the observed outcome is the first
# word sophia msg prints, or the failure it reports.
phase() {
    local name=$1 expected=$2 observed
    shift 2
    observed=$(timeout -s KILL 20 "$sophia" msg --socket "$socket" "$@" 2>&1 | head -n 1 | cut -d: -f1) || true
    if [[ "$observed" == "$expected" ]]; then
        record "$name" "$expected" "$observed" true
    else
        record "$name" "$expected" "${observed:-no reply}" false
        failed=true
    fi
}

drive() {
    local deadline=$((SECONDS + 90))
    socket=
    while [[ -z "$socket" ]]; do
        socket=$(find "${XDG_RUNTIME_DIR:?}" -maxdepth 2 -name control.sock -user "$(id -u)" \
            -newer "$evidence/candidate.txt" 2>/dev/null | head -n 1)
        ((SECONDS < deadline)) || { record startup control-socket "none" false; return 1; }
        sleep 1
    done
    # Ready means the WM configured and control advertises the session operations.
    until "$sophia" msg --socket "$socket" commands 2>/dev/null | grep -q 'reload-profile'; do
        ((SECONDS < deadline)) || { record startup ready "not ready" false; return 1; }
        sleep 1
    done
    record startup ready ready true
    sleep 5
    failed=false
    phase restart-wm completed session restart-wm
    phase reload-unchanged unchanged session reload-profile
    cp -- "$evidence/desktop-rejected.kdl" "$evidence/desktop.kdl"
    phase reload-rejected rejected session reload-profile
    cp -- "$evidence/desktop-valid.kdl" "$evidence/desktop.kdl"
    phase reload-restored unchanged session reload-profile
    phase restart-after-rollback completed session restart-wm
    phase logout completed session logout
}

# The session owns this terminal in the foreground; the driver runs beside it
# and ends it with logout. A driver failure still logs out, so the tty returns.
(
    drive || true
    sleep 30
    "$sophia" msg --socket "${socket:-/nonexistent}" session logout >/dev/null 2>&1 || true
) &
driver=$!

unset SOPHIA_RUN_REAL_ATOMIC_SCANOUT_SMOKE SOPHIA_HAGIA_PROFILE_MODE
export SOPHIA_INSTALL_PREFIX=/opt/sophia-niltempus-desktop
export SOPHIA_DESKTOP_PROFILE="$evidence/desktop.kdl"
export SOPHIA_HAGIA_BIN="$hagia"
export SOPHIA_INSTALLED_ATTEMPT_MODE=hagia
export PATH="/opt/sophia-niltempus-desktop/bin:$PATH"
status=0
"$release/bin/sophia-hagia-session" "--wm-process=$hagia" "--wm-transport=$wire" \
    >"$evidence/session.log" 2>&1 || status=$?
wait "$driver" || true
printf 'session_exit=%s\n' "$status" >>"$evidence/candidate.txt"

passed=$(grep -c '"pass":true' "$phases" 2>/dev/null || true)
total=$(wc -l <"$phases" 2>/dev/null || echo 0)
echo "rehearsal $wire: $passed/$total phases passed, session exit $status"
echo "evidence: $evidence"
[[ "$passed" == "$total" && "$total" -gt 0 && "$status" == 0 ]]
