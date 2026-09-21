#!/usr/bin/env bash
set -euo pipefail

SCRIPT_PATH="$(readlink -f "${BASH_SOURCE[0]}")"
RELEASE_DIR="$(cd "$(dirname "$SCRIPT_PATH")/.." && pwd)"
VERIFY_CHROME="${SOPHIA_VERIFY_NATIVE_CHROME_CORE_BIN:-$RELEASE_DIR/bin/sophia-verify-native-chrome-core}"
if [[ ! -x "$VERIFY_CHROME" ]]; then
    VERIFY_CHROME="$RELEASE_DIR/tools/verify_sophia_native_chrome.sh"
fi

(( $# == 4 )) || {
    echo "usage: $0 SESSION_LOG SEQUENCE_LOG GUARD_LOG RECOVERY_LOG" >&2
    exit 1
}
session_log="$1"
sequence_log="$2"
guard_log="$3"
recovery_log="$4"

fail() {
    echo "installed native-chrome verification failed: $*" >&2
    exit 1
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

"$VERIFY_CHROME" "$session_log" "$sequence_log"
[[ -s "$guard_log" && -s "$recovery_log" ]] ||
    fail "input-guard or recovery evidence is missing"
grep -Fxq 'sophia_session_input_guard schema=1 status=armed' "$guard_log" ||
    fail "the independent input guard was not armed"
if grep -Fxq 'sophia_session_input_guard schema=1 status=triggered' "$guard_log"; then
    fail "the proof used emergency recovery"
fi
recovery="$({
    grep -E '^sophia_tty_recovery schema=3 profile=native ' "$recovery_log" || true
} | tail -n 1)"
[[ -n "$recovery" ]] || fail "normal native TTY recovery is missing"
for pair in \
    termios_restored=true \
    emergency=false \
    session_shutdown=not_requested \
    session_exit_status=none; do
    require_eq "$recovery" "${pair%%=*}" "${pair#*=}"
done
kd_mode_before="$(field "$recovery" kd_mode_before)" ||
    fail "recovery is missing kd_mode_before"
require_eq "$recovery" kd_mode_after "$kd_mode_before"

grep -Eq '^sophia_live_session_startup schema=2 status=output_baseline_ready outputs=2/2$' \
    "$session_log" || fail "both outputs did not establish a startup baseline"
grep -Eq '(^|[[:space:]])sophia_live_native_page_flip schema=1 status=retired output=[0-9]+ ' \
    "$session_log" || fail "no asynchronous native page flip retired"
grep -Eq '^sophia_live_wm schema=1 status=session_action_committed .* action=Logout$' \
    "$session_log" || fail "normal native logout was not committed"
grep -Eq '^sophia_live_session_native_suspend schema=2 outcome=drained drained=true abandoned_scanouts=0 skipped_present=none$' \
    "$session_log" || fail "native presentation did not drain"
grep -Eq '^sophia_live_session_protocol_errors schema=1 expected=[0-9]+ unexpected=0$' \
    "$session_log" || fail "unexpected X11 errors were reported"
grep -Eq '^sophia_live_wm_chrome schema=1 status=negotiated source=wm_policy capability=true clearance=2$' \
    "$session_log" || fail "the WM did not negotiate native chrome ownership"
output_count="$(grep -Ec '^sophia_live_output schema=1 status=complete output=[0-9]+ .*nonzero_exports=[1-9][0-9]*$' "$session_log" || true)"
(( output_count == 2 )) || fail "expected two visible output summaries; found $output_count"

mapfile -t completions < <(
    grep -E '^sophia_live_session schema=(15|16|18) status=bounded_complete ' \
        "$session_log" || true
)
(( ${#completions[@]} == 1 )) ||
    fail "expected one supported completion; found ${#completions[@]}"
completion="${completions[0]}"
for pair in \
    physical_input=enabled \
    wm_policy=external \
    wm_restarts=0 \
    wm_degraded=false \
    native_callback_rejected=0 \
    native_in_flight=false; do
    require_eq "$completion" "${pair%%=*}" "${pair#*=}"
done
physical_keys_routed="$(field "$completion" physical_keys_routed)" ||
    fail "completion is missing physical_keys_routed"
[[ "$physical_keys_routed" =~ ^[1-9][0-9]*$ ]] ||
    fail "the proof routed no physical keys"

echo "installed native-chrome session passed: outputs=$output_count physical_keys_routed=$physical_keys_routed"
