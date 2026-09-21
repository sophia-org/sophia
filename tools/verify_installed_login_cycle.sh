#!/usr/bin/env bash
set -euo pipefail

STATE_HOME="${XDG_STATE_HOME:-$HOME/.local/state}"
LOG_DIR="${SOPHIA_HAGIA_LOG_DIR:-$STATE_HOME/sophia/hagia-session}"
SESSION_LOG="${1:-$LOG_DIR/session.log}"
GUARD_LOG="${2:-$LOG_DIR/input-guard.log}"
RECOVERY_LOG="${3:-$LOG_DIR/recovery.log}"

fail() {
    echo "installed login-cycle verification failed: $*" >&2
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
    '^sophia_session_app schema=1 status=started id=terminal source=startup$' \
    "$SESSION_LOG" "automatic Kitty startup is missing"
require_line \
    '^sophia_live_session_startup schema=2 status=output_baseline_ready outputs=2/2$' \
    "$SESSION_LOG" "both outputs did not establish a startup baseline"
startup="$({
    grep -E '^sophia_live_session_startup schema=2 status=ready ' "$SESSION_LOG" || true
} | head -n 1)"
[[ -n "$startup" ]] || fail "startup readiness is missing"
require_eq "$startup" outputs_ready 2/2
startup_msec="$(field "$startup" elapsed_msec)" ||
    fail "startup readiness is missing elapsed_msec"
[[ "$startup_msec" =~ ^[0-9]+$ ]] && (( startup_msec <= 8000 )) ||
    fail "startup readiness took ${startup_msec:-unknown}ms (limit: 8000ms)"
require_line \
    '^sophia_live_wm schema=1 status=session_action_committed .* action=Logout$' \
    "$SESSION_LOG" "normal logout was not committed"
require_line \
    '^sophia_live_session_health schema=1 status=clean .* pending_wm=0 pending_actions=0 pending_input=0 wm_degraded=false$' \
    "$SESSION_LOG" "final session health is not clean"
require_line \
    '^sophia_live_session_protocol_errors schema=1 expected=[0-9]+ unexpected=0$' \
    "$SESSION_LOG" "unexpected X11 errors were reported"
require_line \
    '^sophia_live_session_native_suspend schema=2 outcome=drained drained=true abandoned_scanouts=0 skipped_present=none$' \
    "$SESSION_LOG" "native presentation did not drain"
require_line \
    '^sophia_live_session_cleanup schema=1 status=clean app_groups=0([[:space:]]|$)' \
    "$SESSION_LOG" "application cleanup did not drain"

output_count="$(grep -Ec \
    '^sophia_live_output schema=1 status=complete output=[0-9]+ .*nonzero_exports=[1-9][0-9]*$' \
    "$SESSION_LOG" || true)"
(( output_count == 2 )) || fail "expected two clean output summaries; found $output_count"
require_line '(^|[[:space:]])sophia_live_native_page_flip schema=1 status=retired output=[0-9]+ ' \
    "$SESSION_LOG" "no native page flip retired"

mapfile -t completions < <(
    grep -E '^sophia_live_session schema=(14|15|16|18) status=bounded_complete ' \
        "$SESSION_LOG" || true
)
(( ${#completions[@]} == 1 )) ||
    fail "expected one supported completion; found ${#completions[@]}"
completion="${completions[0]}"
require_eq "$completion" physical_input enabled
require_eq "$completion" wm_policy external
require_eq "$completion" wm_restarts 0
require_eq "$completion" wm_degraded false
require_eq "$completion" native_submit_failures 0
require_eq "$completion" native_retire_failures 0
require_eq "$completion" native_callback_rejected 0
require_eq "$completion" native_in_flight false
require_eq "$completion" native_cleanup_pending false
require_positive "$completion" physical_keys_routed

require_line '^sophia_session_input_guard schema=1 status=armed$' \
    "$GUARD_LOG" "input guard was not armed"
if grep -Eq '^sophia_session_input_guard schema=1 status=triggered$' "$GUARD_LOG"; then
    fail "cycle used emergency recovery instead of normal logout"
fi
recovery="$({
    grep -E '^sophia_tty_recovery schema=3 profile=hagia ' "$RECOVERY_LOG" || true
} | tail -n 1)"
[[ -n "$recovery" ]] || fail "normal TTY recovery is missing"
require_eq "$recovery" termios_restored true
require_eq "$recovery" emergency false
require_eq "$recovery" session_shutdown not_requested
require_eq "$recovery" session_exit_status none
kd_before="$(field "$recovery" kd_mode_before)" || fail "recovery is missing kd_mode_before"
kd_after="$(field "$recovery" kd_mode_after)" || fail "recovery is missing kd_mode_after"
[[ "$kd_before" == "$kd_after" ]] ||
    fail "KD mode was not restored: before=$kd_before after=$kd_after"

echo "installed Sophia login cycle passed: startup_msec=$startup_msec outputs=$output_count"
