#!/usr/bin/env bash
set -euo pipefail

STATE_HOME="${XDG_STATE_HOME:-$HOME/.local/state}"
LOG_DIR="${SOPHIA_HAGIA_LOG_DIR:-$STATE_HOME/sophia/hagia-session}"
SESSION_LOG="${1:-$LOG_DIR/session.log}"
GUARD_LOG="${2:-$LOG_DIR/input-guard.log}"
RECOVERY_LOG="${3:-$LOG_DIR/recovery.log}"

fail() {
    echo "installed xterm verification failed: $*" >&2
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
line_number() {
    grep -nEm1 "$1" "$2" | cut -d: -f1
}
line_number_after() {
    local pattern="$1" minimum="$2"
    grep -nE "$pattern" "$SESSION_LOG" |
        cut -d: -f1 |
        awk -v minimum="$minimum" '$1 > minimum { print; exit }'
}
parse_geometry() {
    local geometry="$1" prefix="$2"
    [[ "$geometry" =~ ^([0-9]+)x([0-9]+)_(-?[0-9]+)_(-?[0-9]+)$ ]] ||
        fail "malformed $prefix geometry: $geometry"
    printf -v "${prefix}_width" '%d' "${BASH_REMATCH[1]}"
    printf -v "${prefix}_height" '%d' "${BASH_REMATCH[2]}"
    printf -v "${prefix}_x" '%d' "${BASH_REMATCH[3]}"
    printf -v "${prefix}_y" '%d' "${BASH_REMATCH[4]}"
}

require_file "$SESSION_LOG"
require_file "$GUARD_LOG"
require_file "$RECOVERY_LOG"

if grep -Eqi '(^Error:|panicked at|^sophia_[^[:space:]]+ .*status=(failed|degraded)([[:space:]]|$))' \
    "$SESSION_LOG"; then
    fail "session log contains a Sophia error, panic, or degraded status"
fi
require_line \
    '^sophia_live_session schema=7 status=running .*terminal=xterm .*native_presentation=enabled .*physical_input=enabled .*wm_policy=external ' \
    "$SESSION_LOG" "the installed session did not run xterm"
require_line \
    '^sophia_session_app schema=1 status=started id=terminal source=startup$' \
    "$SESSION_LOG" "startup xterm was not launched"
require_line \
    '^sophia_live_outputs schema=2 status=ready discovered=2 presentation=2 native_owned=2 multi_output_scanout=enabled ' \
    "$SESSION_LOG" "two-output native ownership was not established"
require_line \
    '^sophia_live_session_startup schema=2 status=output_baseline_ready outputs=2/2$' \
    "$SESSION_LOG" "both outputs did not establish a startup baseline"
require_line \
    '^sophia_live_session_startup schema=2 status=ready .*outputs_ready=2/2 ' \
    "$SESSION_LOG" "two-output startup readiness is missing"
startup="$(grep -E '^sophia_live_session_startup schema=2 status=ready ' "$SESSION_LOG" | head -n 1)"
startup_msec="$(field "$startup" elapsed_msec)" || fail "startup readiness omitted elapsed_msec"
[[ "$startup_msec" =~ ^[0-9]+$ ]] && (( startup_msec <= 8000 )) ||
    fail "startup readiness took ${startup_msec:-unknown}ms (limit: 8000ms)"

for output in 1 2; do
    mapfile -t work_lines < <(
        grep -E "^sophia_live_work_area schema=1 status=applied output=${output} " \
            "$SESSION_LOG" || true
    )
    (( ${#work_lines[@]} == 1 )) ||
        fail "expected one applied work area for output $output; found ${#work_lines[@]}"
    full="$(field "${work_lines[0]}" full)" || fail "output $output omitted full geometry"
    work="$(field "${work_lines[0]}" work)" || fail "output $output omitted work geometry"
    parse_geometry "$full" full
    parse_geometry "$work" work
    (( work_width == full_width
        && work_x == full_x
        && work_y > full_y
        && work_height < full_height
        && work_y + work_height == full_y + full_height )) ||
        fail "output $output work area is not one bounded top reservation"
    if (( output == 1 )); then
        primary_work_width=$work_width
        primary_work_height=$work_height
        primary_work_x=$work_x
        primary_work_y=$work_y
    fi
done
require_line \
    '^sophia_live_work_area schema=1 status=reduced outputs=2 changed=2 rejected=0 active_reservations=1$' \
    "$SESSION_LOG" "Narthex did not reduce both output work areas exactly once"

admission="$({
    grep -E '^sophia_live_surface_admission schema=1 status=frontend_admitted transaction=[0-9]+ surface=[0-9]+$' \
        "$SESSION_LOG" || true
} | head -n 1)"
[[ -n "$admission" ]] || fail "xterm was not admitted"
surface="$(field "$admission" surface)" || fail "xterm admission omitted its surface"
committed="$({
    grep -E "^sophia_live_visual_admission schema=1 status=committed transaction=[0-9]+ surface=${surface} source=cpu_backing_snapshot$" \
        "$SESSION_LOG" || true
} | head -n 1)"
[[ -n "$committed" ]] || fail "xterm did not commit its CPU backing snapshot"
transaction="$(field "$committed" transaction)" || fail "xterm commit omitted its transaction"
candidate="$({
    grep -E "^sophia_live_visual_candidate schema=1 status=selected transaction=${transaction} surface=${surface} width=[0-9]+ height=[0-9]+ evidence=BackingSnapshot$" \
        "$SESSION_LOG" || true
} | head -n 1)"
[[ -n "$candidate" ]] || fail "xterm was not selected from backing-snapshot evidence"
require_line \
    "^sophia_live_visual_candidate_identity schema=1 status=selected transaction=${transaction} surface=${surface} source=cpu_buffer buffer=[1-9][0-9]*$" \
    "$SESSION_LOG" "xterm backing evidence omitted its CPU-buffer identity"
geometry="$({
    grep -E "^sophia_live_visual_admission_geometry schema=1 status=committed transaction=${transaction} surface=${surface} " \
        "$SESSION_LOG" || true
} | head -n 1)"
[[ -n "$geometry" ]] || fail "xterm commit omitted atomic pixel geometry"
source="$(field "$geometry" source)" || fail "xterm commit omitted source geometry"
target="$(field "$geometry" target)" || fail "xterm commit omitted target geometry"
[[ "$source" =~ ^([0-9]+)x([0-9]+)$ ]] || fail "malformed xterm source geometry: $source"
source_width="${BASH_REMATCH[1]}"
source_height="${BASH_REMATCH[2]}"
candidate_width="$(field "$candidate" width)" || fail "xterm candidate omitted its width"
candidate_height="$(field "$candidate" height)" || fail "xterm candidate omitted its height"
(( candidate_width == source_width && candidate_height == source_height )) ||
    fail "xterm candidate and committed source extents differ"
parse_geometry "$target" target
(( source_width == target_width && source_height == target_height )) ||
    fail "xterm pixels do not match the target extent"
# The WM owns the outer work-area placement; X Authority owns the source
# pixels. A symmetric inset proves those independent facts still converge.
left_inset=$((target_x - primary_work_x))
top_inset=$((target_y - primary_work_y))
right_inset=$((primary_work_x + primary_work_width - target_x - target_width))
bottom_inset=$((primary_work_y + primary_work_height - target_y - target_height))
(( left_inset > 0 && left_inset <= 16
    && top_inset > 0 && top_inset <= 16
    && left_inset == right_inset
    && top_inset == bottom_inset )) ||
    fail "xterm target is not symmetrically inset inside the primary work area"

geometry_line="$(line_number "visual_admission_geometry schema=1 status=committed transaction=${transaction} surface=${surface} " "$SESSION_LOG")"
native_submission="$({
    awk -v minimum="$geometry_line" '
        NR > minimum &&
        /sophia_live_native_page_flip schema=1 status=submitted output=1 / &&
        /content=Some\((Cpu|RetainedMixed)/ { print; exit }
    ' "$SESSION_LOG"
} || true)"
[[ -n "$native_submission" ]] || fail "xterm commit did not reach primary native scanout"
submission="$(field "$native_submission" submission)" || fail "native submission omitted its ID"
frame="$(field "$native_submission" frame)" || fail "native submission omitted its frame"
submit_line="$(line_number_after "status=submitted output=1 submission=${submission} .* frame=${frame}$" "$geometry_line")"
retire_line="$(line_number_after "status=retired output=1 submission=${submission} frame=${frame}$" "$submit_line")"
startup_line="$(line_number_after 'sophia_live_session_startup schema=2 status=ready .*presented=true ' "$retire_line")"
[[ -n "$retire_line" && -n "$startup_line" ]] ||
    fail "xterm CPU commit did not retire before startup readiness"

for record in \
    'sophia_live_session_vt schema=4 status=queued target=[0-9]+ modifier_releases=[2-4]' \
    'sophia_live_session_vt schema=4 status=preparing target=[0-9]+' \
    'sophia_live_session_vt schema=6 status=quiesced target=[0-9]+ outcome=drained drained=true abandoned_scanouts=0 skipped_present=none' \
    'sophia_live_session_vt schema=4 status=requested target=[0-9]+' \
    'sophia_live_seat schema=1 status=release_pending' \
    'sophia_live_seat schema=1 status=suspended' \
    'sophia_live_seat schema=1 status=acquire_pending' \
    'sophia_live_seat schema=1 status=active source=resume'; do
    require_line "^${record}$" "$SESSION_LOG" "VT lifecycle record is missing: $record"
done
capture="$({
    grep -E '^sophia_live_renderer_handoff schema=1 status=captured images=0$' \
        "$SESSION_LOG" || true
} | head -n 1)"
restore="$({
    grep -E '^sophia_live_renderer_handoff schema=1 status=restored images=0 source=seat_resume$' \
        "$SESSION_LOG" || true
} | head -n 1)"
[[ -n "$capture" && -n "$restore" ]] ||
    fail "CPU-only xterm handoff unexpectedly required imported renderer images"

queued_line="$(line_number 'schema=4 status=queued target=' "$SESSION_LOG")"
preparing_line="$(line_number 'schema=4 status=preparing target=' "$SESSION_LOG")"
capture_line="$(line_number 'status=captured images=' "$SESSION_LOG")"
quiesced_line="$(line_number 'schema=6 status=quiesced target=' "$SESSION_LOG")"
requested_line="$(line_number 'schema=4 status=requested target=' "$SESSION_LOG")"
release_line="$(line_number 'sophia_live_seat schema=1 status=release_pending$' "$SESSION_LOG")"
suspended_line="$(line_number 'sophia_live_seat schema=1 status=suspended$' "$SESSION_LOG")"
acquire_line="$(line_number 'sophia_live_seat schema=1 status=acquire_pending$' "$SESSION_LOG")"
scene_line="$(line_number_after '^sophia_live_scene_handoff schema=1 status=rehydrated outputs=2 nonzero_outputs=[12] primary_nonzero_pixel_bytes=[1-9][0-9]* source=seat_resume$' "$acquire_line")"
restore_line="$(line_number 'status=restored images=.* source=seat_resume$' "$SESSION_LOG")"
resume_line="$(line_number 'sophia_live_seat schema=1 status=active source=resume$' "$SESSION_LOG")"
flip_line="$(line_number_after 'sophia_live_native_page_flip schema=1 status=retired output=1 ' "$resume_line")"
[[ -n "$scene_line" && -n "$flip_line" ]] ||
    fail "the retained CPU scene did not rehydrate and retire after VT resume"
(( queued_line < preparing_line
    && preparing_line < capture_line
    && capture_line < quiesced_line
    && quiesced_line < requested_line
    && requested_line < release_line
    && release_line < suspended_line
    && suspended_line < acquire_line
    && acquire_line < scene_line
    && scene_line < restore_line
    && restore_line < resume_line
    && resume_line < flip_line )) ||
    fail "VT handoff and retained-scene recovery records are out of order"
if grep -Eq 'status=forced_detach|outcome=forced_detach_|remained in flight during teardown' \
    "$SESSION_LOG"; then
    fail "operator-requested VT switch used the revoked-seat fallback"
fi

require_line '^sophia_live_wm schema=1 status=session_action_committed .* action=Logout$' \
    "$SESSION_LOG" "normal Super-Shift-Q logout was not committed"
require_line '^sophia_live_session_health schema=1 status=clean .*wm_degraded=false$' \
    "$SESSION_LOG" "final session health was not clean"
require_line '^sophia_live_session_protocol_errors schema=1 expected=[0-9]+ unexpected=0$' \
    "$SESSION_LOG" "xterm session emitted an unexpected X protocol error"
require_line '^sophia_live_session_native_suspend schema=2 outcome=drained drained=true abandoned_scanouts=0 skipped_present=none$' \
    "$SESSION_LOG" "native presentation did not drain"
require_line '^sophia_live_session_cleanup schema=1 status=clean app_groups=0 frontend_workers=0 namespace=revoked xauthority=removed$' \
    "$SESSION_LOG" "application or X Authority ownership remained after logout"
for output in 1 2; do
    require_line \
        "^sophia_live_output schema=1 status=complete output=${output} .*nonzero_exports=[1-9][0-9]*$" \
        "$SESSION_LOG" "output $output did not finish with visible exports"
done
completion="$({
    grep -E '^sophia_live_session schema=(14|15|16|18) status=bounded_complete ' \
        "$SESSION_LOG" || true
} | tail -n 1)"
[[ -n "$completion" ]] || fail "bounded session completion is missing"
for assignment in \
    native_presentation=enabled physical_input=enabled wm_policy=external \
    wm_restarts=0 wm_degraded=false native_submit_failures=0 native_retire_failures=0 \
    native_callback_rejected=0 native_in_flight=false native_cleanup_pending=false; do
    key="${assignment%%=*}"
    expected="${assignment#*=}"
    actual="$(field "$completion" "$key")" || fail "completion omitted $key"
    [[ "$actual" == "$expected" ]] || fail "$key is $actual, expected $expected"
done
physical_keys="$(field "$completion" physical_keys_routed)" ||
    fail "completion omitted physical_keys_routed"
[[ "$physical_keys" =~ ^[0-9]+$ ]] && (( physical_keys > 0 )) ||
    fail "xterm proof routed no physical keys"

require_line '^sophia_session_input_guard schema=1 status=armed$' \
    "$GUARD_LOG" "input guard was not armed"
if grep -Eq '^sophia_session_input_guard schema=1 status=triggered$' "$GUARD_LOG"; then
    fail "xterm proof used emergency recovery instead of normal logout"
fi
recovery="$({
    grep -E '^sophia_tty_recovery schema=3 profile=hagia ' "$RECOVERY_LOG" || true
} | tail -n 1)"
[[ -n "$recovery" ]] || fail "normal Hagia TTY recovery is missing"
for assignment in termios_restored=true emergency=false session_shutdown=not_requested session_exit_status=none; do
    key="${assignment%%=*}"
    expected="${assignment#*=}"
    actual="$(field "$recovery" "$key")" || fail "recovery omitted $key"
    [[ "$actual" == "$expected" ]] || fail "$key is $actual, expected $expected"
done
kd_before="$(field "$recovery" kd_mode_before)" || fail "recovery omitted kd_mode_before"
kd_after="$(field "$recovery" kd_mode_after)" || fail "recovery omitted kd_mode_after"
[[ "$kd_before" == "$kd_after" ]] || fail "KD mode was not restored exactly"

echo "installed xterm session passed: surface=$surface target=$target outputs=2"
