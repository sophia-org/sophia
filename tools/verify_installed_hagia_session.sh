#!/usr/bin/env bash
set -euo pipefail

session_log="${1:-}"
guard_log="${2:-}"
recovery_log="${3:-}"
[[ -s "$session_log" && -s "$guard_log" && -s "$recovery_log" ]] || {
    echo "usage: tools/verify_installed_hagia_session.sh SESSION_LOG GUARD_LOG RECOVERY_LOG" >&2
    exit 1
}
fail() { echo "installed Hagia session verification failed: $*" >&2; exit 1; }
require() { grep -Eq "$1" "$2" || fail "$3"; }
field() {
    local line="$1" key="$2" token
    for token in $line; do
        [[ "$token" != "$key="* ]] || { printf '%s\n' "${token#*=}"; return; }
    done
    return 1
}

if grep -Eqi '(^Error:|panicked at|^sophia_[^[:space:]]+ .*status=(failed|degraded)([[:space:]]|$))' "$session_log"; then
    fail "session log contains an error, panic, or degraded status"
fi
if grep -Eq '^sophia_live_session_startup_proof schema=1 status=not_requested$' "$session_log"; then
    require '^sophia_live_session schema=1 status=desktop_ready startup_apps=[0-9]+$' \
        "$session_log" "normal desktop admission is missing"
    require '^sophia_live_outputs schema=2 status=ready ' "$session_log" "output initialization is missing"
    outputs_ready="not_application_proof"
else
require '^sophia_session_app schema=(1|2) status=started id=terminal source=startup$' \
    "$session_log" "automatic terminal startup is missing"
startup="$(grep -E '^sophia_live_session_startup schema=2 status=ready ' "$session_log" | head -n 1 || true)"
[[ -n "$startup" ]] || fail "startup readiness is missing"
outputs_ready="$(field "$startup" outputs_ready || true)"
[[ "$outputs_ready" =~ ^[1-9][0-9]*/[1-9][0-9]*$ \
    && "${outputs_ready%/*}" == "${outputs_ready#*/}" ]] ||
    fail "startup did not settle every positive output: ${outputs_ready:-missing}"
fi
require '^sophia_live_wm schema=1 status=session_action_committed .* action=Logout$' \
    "$session_log" "normal logout was not committed"
require '^sophia_live_session_health schema=1 status=clean .*pending_wm=0 .*pending_actions=0 .*pending_input=0 .*wm_degraded=false$' \
    "$session_log" "final session health is not clean"
require '^sophia_live_layout_health schema=2 status=clean ' \
    "$session_log" "final layout health is not clean"
require '^sophia_live_session_protocol_errors schema=1 expected=[0-9]+ unexpected=0$' \
    "$session_log" "unexpected X11 errors were reported"
require '^sophia_live_session_native_suspend schema=2 outcome=drained drained=true abandoned_scanouts=0 skipped_present=none$' \
    "$session_log" "native presentation did not drain"
require '^sophia_live_session_cleanup schema=1 status=clean app_groups=0([[:space:]]|$)' \
    "$session_log" "application cleanup did not drain"
completion="$(grep -E '^sophia_live_session schema=(14|15|16|17|18|19) status=bounded_complete ' "$session_log" | tail -n 1 || true)"
[[ -n "$completion" ]] || fail "supported completion is missing"
for assignment in 'physical_input=enabled' 'wm_policy=external' 'wm_degraded=false' \
    'native_submit_failures=0' 'native_retire_failures=0' \
    'native_callback_rejected=0' 'native_in_flight=false' \
    'native_cleanup_pending=false'; do
    [[ " $completion " == *" $assignment "* ]] || fail "completion violates $assignment"
done
require '^sophia_session_input_guard schema=1 status=armed$' "$guard_log" \
    "input guard was not armed"
grep -Eq '^sophia_session_input_guard schema=1 status=triggered$' "$guard_log" &&
    fail "normal session used emergency recovery"
recovery="$(grep -E '^sophia_tty_recovery schema=3 profile=hagia ' "$recovery_log" | tail -n 1 || true)"
[[ -n "$recovery" && " $recovery " == *' termios_restored=true '* \
    && " $recovery " == *' emergency=false '* \
    && " $recovery " == *' session_shutdown=not_requested '* ]] ||
    fail "normal Hagia TTY recovery is invalid"
kd_before="$(field "$recovery" kd_mode_before || true)"
kd_after="$(field "$recovery" kd_mode_after || true)"
[[ -n "$kd_before" && "$kd_before" == "$kd_after" ]] || fail "KD mode was not restored"

echo "installed Hagia session verified: outputs_ready=$outputs_ready"
