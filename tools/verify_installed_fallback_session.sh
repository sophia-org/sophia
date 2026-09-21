#!/usr/bin/env bash
set -euo pipefail

STATE_HOME="${XDG_STATE_HOME:-$HOME/.local/state}"
LOG_DIR="${SOPHIA_KITTY_LOG_DIR:-$STATE_HOME/sophia/kitty-session}"
SESSION_LOG="${1:-$LOG_DIR/session.log}"
GUARD_LOG="${2:-$LOG_DIR/input-guard.log}"
RECOVERY_LOG="${3:-$LOG_DIR/recovery.log}"

fail() {
    echo "installed fallback verification failed: $*" >&2
    exit 1
}
require_file() {
    [[ -s "$1" ]] || fail "missing or empty evidence file: $1"
}
require_line() {
    grep -Eq "$1" "$2" || fail "$3 ($2)"
}
field() {
    local line="$1" key="$2" token
    for token in $line; do
        if [[ "$token" == "$key="* ]]; then
            printf '%s\n' "${token#*=}"
            return 0
        fi
    done
    return 1
}
require_eq() {
    local line="$1" key="$2" expected="$3" actual
    actual="$(field "$line" "$key")" || fail "record is missing $key"
    [[ "$actual" == "$expected" ]] ||
        fail "$key is $actual, expected $expected"
}
require_positive() {
    local line="$1" key="$2" actual
    actual="$(field "$line" "$key")" || fail "record is missing $key"
    [[ "$actual" =~ ^[0-9]+$ ]] && (( actual > 0 )) ||
        fail "$key did not record positive activity"
}

require_file "$SESSION_LOG"
require_file "$GUARD_LOG"
require_file "$RECOVERY_LOG"

if grep -Eqi '(^Error:|panicked at|^sophia_[^[:space:]]+ .*status=(failed|degraded)([[:space:]]|$))' \
    "$SESSION_LOG"; then
    fail "session log contains a Sophia error, panic, or degraded status"
fi
require_line \
    '^sophia_live_session_mode schema=1 mode=normal configured_apps=1 startup_apps=1$' \
    "$SESSION_LOG" "fallback did not start the bounded single-application profile"
require_line \
    '^sophia_live_session schema=[0-9]+ status=running .* wm_policy=disabled ' \
    "$SESSION_LOG" "fallback unexpectedly enabled WM policy"
require_line \
    '^sophia_session_app schema=1 status=started id=terminal source=startup$' \
    "$SESSION_LOG" "automatic fallback Kitty startup is missing"
require_line \
    '^sophia_session_app schema=1 status=exited id=terminal source=startup exit_status=exit status: 0$' \
    "$SESSION_LOG" "fallback Kitty did not exit normally"
require_line \
    '^sophia_live_outputs schema=2 status=ready discovered=2 presentation=2 native_owned=2 multi_output_scanout=enabled ' \
    "$SESSION_LOG" "fallback did not own both installed outputs"
require_line \
    '^sophia_live_session_startup schema=2 status=output_baseline_ready outputs=2/2$' \
    "$SESSION_LOG" "both outputs did not establish a startup baseline"
mapfile -t startup_outputs < <(
    grep -E '^sophia_live_native_startup_output schema=1 status=presented output=[0-9]+ proof=synchronous_modeset submission=1$' \
        "$SESSION_LOG"
)
(( ${#startup_outputs[@]} == 2 )) ||
    fail "expected two synchronously presented startup outputs"
[[ "$(printf '%s\n' "${startup_outputs[@]}" | sed -n 's/.* output=\([0-9][0-9]*\) .*/\1/p' | sort -u | wc -l)" == 2 ]] ||
    fail "startup output evidence contains duplicate output identities"
# A damage-idle output keeps its proven startup modeset. Requiring a redundant
# flip there would defeat unchanged-frame suppression; active content must
# still retire asynchronously somewhere in the session.
require_line \
    '(^|[[:space:]])sophia_live_native_page_flip schema=1 status=retired output=[0-9]+ ' \
    "$SESSION_LOG" "no asynchronous page flip retired"
require_line \
    '^sophia_live_session_startup schema=2 status=content_ready source=stable_present_scanout nonzero_rgb_pixels=[1-9][0-9]*$' \
    "$SESSION_LOG" "fallback never retired visible Kitty pixels"

startup="$({
    grep -E '^sophia_live_session_startup schema=2 status=ready ' "$SESSION_LOG" || true
} | head -n 1)"
[[ -n "$startup" ]] || fail "startup readiness is missing"
require_eq "$startup" outputs_ready 2/2
startup_msec="$(field "$startup" elapsed_msec)" ||
    fail "startup readiness is missing elapsed_msec"
[[ "$startup_msec" =~ ^[0-9]+$ ]] && (( startup_msec <= 8000 )) ||
    fail "startup readiness took ${startup_msec:-unknown}ms (limit: 8000ms)"

for output in 1 2; do
    require_line \
        "^sophia_live_output schema=1 status=complete output=${output} .*nonzero_exports=[1-9][0-9]*$" \
        "$SESSION_LOG" "fallback output $output has no visible export summary"
done
require_line \
    '^sophia_live_session_protocol_errors schema=1 expected=[0-9]+ unexpected=0$' \
    "$SESSION_LOG" "unexpected X11 errors were reported"
require_line \
    '^sophia_live_session_health schema=1 status=clean .* pending_wm=0 pending_actions=0 pending_input=0 wm_degraded=false$' \
    "$SESSION_LOG" "final fallback health is not clean"
require_line \
    '^sophia_live_session_native_suspend schema=2 outcome=drained drained=true abandoned_scanouts=0 skipped_present=none$' \
    "$SESSION_LOG" "native presentation did not drain"
require_line \
    '^sophia_live_session_cleanup schema=1 status=clean app_groups=0([[:space:]]|$)' \
    "$SESSION_LOG" "fallback application cleanup did not drain"

completion="$({
    grep -E '^sophia_live_session schema=(14|15|16|18) status=bounded_complete ' \
        "$SESSION_LOG" || true
} | tail -n 1)"
[[ -n "$completion" ]] || fail "fallback completion record is missing"
for pair in \
    physical_input=enabled \
    wm_policy=disabled \
    native_submit_failures=0 \
    native_retire_failures=0 \
    native_callback_rejected=0 \
    native_in_flight=false \
    native_cleanup_pending=false; do
    require_eq "$completion" "${pair%%=*}" "${pair#*=}"
done
require_positive "$completion" physical_keys_routed

require_line '^sophia_session_input_guard schema=1 status=armed$' \
    "$GUARD_LOG" "independent input guard was not armed"
if grep -Eq '^sophia_session_input_guard schema=1 status=triggered$' "$GUARD_LOG"; then
    fail "fallback used emergency recovery"
fi
recovery="$({
    grep -E '^sophia_tty_recovery schema=3 profile=kitty ' "$RECOVERY_LOG" || true
} | tail -n 1)"
[[ -n "$recovery" ]] || fail "Kitty TTY recovery is missing"
for pair in \
    termios_restored=true \
    emergency=false \
    session_shutdown=not_requested \
    session_exit_status=none; do
    require_eq "$recovery" "${pair%%=*}" "${pair#*=}"
done
kd_before="$(field "$recovery" kd_mode_before)" || fail "recovery is missing kd_mode_before"
kd_after="$(field "$recovery" kd_mode_after)" || fail "recovery is missing kd_mode_after"
[[ "$kd_before" == "$kd_after" ]] || fail "KD mode was not restored"

echo "installed Sophia fallback passed: startup_msec=$startup_msec outputs=2"
