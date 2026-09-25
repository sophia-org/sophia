#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEMP_DIR="$(mktemp -d)"
trap 'rm -rf -- "$TEMP_DIR"' EXIT

state_home="$TEMP_DIR/state"
runtime_dir="$TEMP_DIR/runtime"
prefix="$TEMP_DIR/prefix"
release="$prefix/releases/diagnostic-test"
mkdir -p "$state_home" "$runtime_dir" "$release"
chmod 700 "$state_home" "$runtime_dir"
session_state="$state_home/sophia/native-session"
install -d -m 700 "$session_state"
printf 'prior lifecycle\n' >"$session_state/lifecycle.log"
printf 'prior guard\n' >"$session_state/input-guard.log"
printf 'prior recovery\n' >"$session_state/recovery.log"
printf 'prior session\n' >"$session_state/session.log"

set +e
env \
    XDG_STATE_HOME="$state_home" \
    XDG_RUNTIME_DIR="$runtime_dir" \
    SOPHIA_INSTALLED_SESSION=true \
    SOPHIA_INSTALLED_VERSION=0.1.0 \
    SOPHIA_INSTALLED_COMMIT=0123456789abcdef \
    SOPHIA_BUILD_SESSION=false \
    SOPHIA_MANAGE_KEYD=false \
    SOPHIA_TTY_PROFILE=native \
    SOPHIA_BIN="${SOPHIA_BIN:-${CARGO_TARGET_DIR:-$ROOT_DIR/target}/debug/sophia}" \
    "$ROOT_DIR/tools/run_sophia_session.sh" \
    </dev/null >"$TEMP_DIR/runner.out" 2>"$TEMP_DIR/runner.err"
runner_status=$?
set -e
[[ "$runner_status" == 1 ]]

lifecycle="$state_home/sophia/native-session/lifecycle.log"
grep -Fxq 'prior lifecycle' "$lifecycle.previous"
grep -Fxq 'prior guard' "$session_state/input-guard.log.previous"
grep -Fxq 'prior recovery' "$session_state/recovery.log.previous"
grep -Fxq 'prior session' "$session_state/session.log.previous"
[[ -f "$session_state/input-guard.log" && ! -s "$session_state/input-guard.log" ]]
[[ -f "$session_state/recovery.log" && ! -s "$session_state/recovery.log" ]]
[[ -f "$session_state/session.log" && ! -s "$session_state/session.log" ]]
[[ "$(stat -c %a "$session_state/input-guard.log")" == 600 ]]
[[ "$(stat -c %a "$session_state/recovery.log")" == 600 ]]
[[ "$(stat -c %a "$session_state/session.log")" == 600 ]]
grep -Fxq \
    'sophia_session_diagnostic schema=1 status=failed phase=preflight installed=true version=0.1.0 commit=0123456789abcdef exit_status=1' \
    "$lifecycle"

printf 'schema=1\nversion=0.1.0\ncommit=0123456789abcdef\nrelease_id=diagnostic-test\n' \
    >"$release/manifest"
(
    cd "$release"
    sha256sum manifest >SHA256SUMS
)
ln -s releases/diagnostic-test "$prefix/current"

status_output="$(
    env \
        XDG_STATE_HOME="$state_home" \
        SOPHIA_INSTALL_PREFIX="$prefix" \
        "$ROOT_DIR/tools/status_live_session.sh"
)"
grep -Fq \
    'sophia_install_status schema=1 prefix=' \
    <<<"$status_output"
grep -Fq \
    'sophia_session_diagnostic schema=1 status=failed phase=preflight installed=true version=0.1.0 commit=0123456789abcdef exit_status=1' \
    <<<"$status_output"
[[ "$(
    grep -Fc \
        'sophia_session_diagnostic schema=1 status=failed phase=preflight installed=true version=0.1.0 commit=0123456789abcdef exit_status=1' \
        <<<"$status_output"
)" == 1 ]]

source "$ROOT_DIR/tools/lib/session_lifecycle.sh"
rotation_log="$TEMP_DIR/rotation.log"
printf 'first generation\n' >"$rotation_log"
sophia_session_rotate_log "$rotation_log"
grep -Fxq 'first generation' "$rotation_log.previous"
[[ -f "$rotation_log" && ! -s "$rotation_log" ]]
printf 'second generation\n' >"$rotation_log"
sophia_session_rotate_log "$rotation_log"
grep -Fxq 'second generation' "$rotation_log.previous"
[[ -f "$rotation_log" && ! -s "$rotation_log" ]]
[[ "$(stat -c %a "$rotation_log")" == 600 ]]
if sophia_session_rotate_log; then
    echo "session log rotation accepted a missing path" >&2
    exit 1
fi
phase_log="$TEMP_DIR/phases.log"
for phase in preflight input_guard graphics_takeover session handoff; do
    sophia_session_record_failure \
        "$phase_log" "$phase" false unknown unknown 9
done
for phase in preflight input_guard graphics_takeover session handoff; do
    grep -Fxq \
        "sophia_session_diagnostic schema=1 status=failed phase=$phase installed=false version=unknown commit=unknown exit_status=9" \
        "$phase_log"
done
if sophia_session_record_failure \
    "$TEMP_DIR/invalid.log" invalid true 0.1.0 0123456789abcdef 1; then
    echo "lifecycle diagnostic accepted an invalid phase" >&2
    exit 1
fi
[[ ! -e "$TEMP_DIR/invalid.log" ]]

echo "session lifecycle diagnostic checks passed"
