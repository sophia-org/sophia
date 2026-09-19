#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=tools/lib/rendering_performance.sh
source "$ROOT_DIR/tools/lib/rendering_performance.sh"

STATE_HOME="${XDG_STATE_HOME:-${HOME}/.local/state}"
LOG_DIR="${SOPHIA_STANDALONE_LOG_DIR:-$STATE_HOME/sophia/standalone-session}"
SESSION_LOG="${1:-$LOG_DIR/session.log}"

fail() {
    echo "Sophia glxgears performance report failed: $*" >&2
    exit 1
}

positive_field() {
    local line="$1" key="$2" value
    value="$(rendering_performance_field "$line" "$key")" ||
        fail "completion lacks $key"
    [[ "$value" =~ ^[0-9]+$ ]] || fail "$key is not an integer"
    ((value > 0)) || fail "$key is not positive"
    printf '%s\n' "$value"
}

field() {
    local line="$1" key="$2" value
    value="$(rendering_performance_field "$line" "$key")" ||
        fail "completion lacks $key"
    printf '%s\n' "$value"
}

nonnegative_field() {
    local line="$1" key="$2" value
    value="$(rendering_performance_field "$line" "$key")" ||
        fail "completion lacks $key"
    [[ "$value" =~ ^[0-9]+$ ]] || fail "$key is not a nonnegative integer"
    printf '%s\n' "$value"
}

[[ -s "$SESSION_LOG" ]] || fail "missing session log: $SESSION_LOG"

if grep -Eqi \
    '(^Error:|panicked at|admission_group_(invalid|overflowed)|mismatched.transaction|status=(failed|degraded)([[:space:]]|$))' \
    "$SESSION_LOG"; then
    fail "session contains an error, invalid admission group, or degraded status"
fi

benchmark="$(
    grep -E '^sophia_glxgears_benchmark schema=1 ' "$SESSION_LOG" |
        tail -n 1 || true
)"
[[ -n "$benchmark" ]] || fail "missing glxgears benchmark metadata"
client="$(
    grep -E '^sophia_glxgears_client schema=1 status=complete ' "$SESSION_LOG" |
        tail -n 1 || true
)"
[[ -n "$client" ]] || fail "missing bounded glxgears client completion"
completion="$(
    grep -E '^sophia_live_session schema=16 status=bounded_complete ' "$SESSION_LOG" |
        tail -n 1 || true
)"
[[ -n "$completion" ]] || fail "\
missing bounded Sophia session completion.
The session's own records are absent from $SESSION_LOG, which happens when the
session ran as an ordinary one: sophia then diverts its records to the reduced
per-session evidence log, where a cadence summary keeps samples and loses the
mean_fps and p95_frame_msec this report reads. A benchmark session must be
bounded -- tools/run_sophia_session.sh passes --max-runtime-ms for exactly
this -- so its full records stay on stdout and reach this log."
grep -Eq '^sophia_live_session_protocol_errors schema=1 expected=[0-9]+ unexpected=0$' \
    "$SESSION_LOG" || fail "session contains unexpected X11 protocol errors"
grep -Eq '^sophia_live_session_cleanup schema=1 status=clean ' "$SESSION_LOG" ||
    fail "session cleanup was not clean"

for assignment in \
    native_presentation=enabled \
    native_submit_failures=0 \
    native_retire_failures=0 \
    native_in_flight=false \
    native_cleanup_pending=false \
    wm_restarts=0 \
    wm_degraded=false \
    present_live_sources=0 \
    present_live_fences=0 \
    present_live_transactions=0; do
    [[ " $completion " == *" $assignment "* ]] ||
        fail "completion does not contain $assignment"
done

renderer_line="$(
    grep -E '^GL_RENDERER[[:space:]]*=' "$SESSION_LOG" |
        head -n 1 || true
)"
[[ -n "$renderer_line" ]] || fail "missing OpenGL renderer identity"
renderer_identity="${renderer_line#*=}"
renderer_identity="${renderer_identity#"${renderer_identity%%[![:space:]]*}"}"
[[ -n "$renderer_identity" ]] || fail "OpenGL renderer identity is empty"
renderer_sha256="$(printf '%s' "$renderer_identity" | sha256sum | awk '{print $1}')"

output_repaint="$(
    grep -E '^.*sophia_live_output_repaint schema=1 status=[^ ]+ output=1 mode=full ' \
        "$SESSION_LOG" |
        head -n 1 || true
)"
output_pixels="$(
    rendering_performance_field "$output_repaint" pixels 2>/dev/null || true
)"
[[ "$output_pixels" =~ ^[1-9][0-9]*$ ]] ||
    fail "missing positive output pixel count"

cadence="$(
    grep -E '^sophia_live_present_cadence schema=1 status=complete ' \
        "$SESSION_LOG" | tail -n 1 || true
)"
[[ -n "$cadence" ]] || fail "\
missing retained-buffer cadence summary (see the bounded-session note above)"
timestamp_count="$(rendering_performance_field "$cadence" samples)" ||
    fail "cadence summary lacks samples"
advancing_intervals="$(rendering_performance_field "$cadence" advancing_intervals)" ||
    fail "cadence summary lacks advancing_intervals"
nonadvancing="$(rendering_performance_field "$cadence" nonadvancing)" ||
    fail "cadence summary lacks nonadvancing"
overflowed="$(rendering_performance_field "$cadence" overflowed)" ||
    fail "cadence summary lacks overflowed"
present_fps="$(rendering_performance_field "$cadence" mean_fps)" ||
    fail "cadence summary lacks mean_fps"
p95_msec="$(rendering_performance_field "$cadence" p95_frame_msec)" ||
    fail "cadence summary lacks p95_frame_msec"
[[ "$timestamp_count" =~ ^[0-9]+$
    && "$advancing_intervals" =~ ^[0-9]+$
    && "$nonadvancing" =~ ^[0-9]+$ ]] ||
    fail "cadence counts must be nonnegative integers"
((timestamp_count >= 3 && advancing_intervals == timestamp_count - 1)) ||
    fail "cadence summary needs at least three exact advancing samples"
((nonadvancing == 0)) || fail "cadence summary contains nonadvancing timestamps"
[[ "$overflowed" == false ]] || fail "cadence summary overflowed"
awk -v fps="$present_fps" -v p95="$p95_msec" \
    'BEGIN { exit !(fps >= 55 && p95 > 0 && p95 <= 25) }' ||
    fail "cadence under pointer motion must remain at least 55 FPS with p95 at most 25 ms"

duration_seconds="$(rendering_performance_field "$benchmark" duration_seconds)" ||
    fail "benchmark metadata lacks duration_seconds"
surface_width="$(rendering_performance_field "$benchmark" surface_width)" ||
    fail "benchmark metadata lacks surface_width"
surface_height="$(rendering_performance_field "$benchmark" surface_height)" ||
    fail "benchmark metadata lacks surface_height"
swap_interval="$(rendering_performance_field "$benchmark" swap_interval)" ||
    fail "benchmark metadata lacks swap_interval"
client_samples="$(rendering_performance_field "$client" samples)" ||
    fail "client completion lacks samples"
client_mean_fps="$(rendering_performance_field "$client" mean_fps)" ||
    fail "client completion lacks mean_fps"
client_duration="$(rendering_performance_field "$client" duration_seconds)" ||
    fail "client completion lacks duration_seconds"
client_timed_exit="$(rendering_performance_field "$client" timed_exit)" ||
    fail "client completion lacks timed_exit"

[[ "$duration_seconds" =~ ^[1-9][0-9]*$
    && "$surface_width" =~ ^[1-9][0-9]*$
    && "$surface_height" =~ ^[1-9][0-9]*$
    && "$client_samples" =~ ^[1-9][0-9]*$ ]] ||
    fail "benchmark dimensions, duration, and client samples must be positive"
[[ "$swap_interval" == 1 ]] || fail "benchmark did not use swap interval 1"
[[ "$client_duration" == "$duration_seconds" ]] ||
    fail "client duration does not match benchmark metadata"
[[ "$client_timed_exit" == true ]] || fail "client did not complete its bounded run"
awk -v fps="$client_mean_fps" 'BEGIN { exit !(fps > 0) }' ||
    fail "client mean FPS must be positive"

native_retirements="$(positive_field "$completion" native_retirements)"
native_nonzero_exports="$(positive_field "$completion" native_nonzero_exports)"
native_mixed_exports="$(positive_field "$completion" native_mixed_exports)"
present_complete_copy="$(positive_field "$completion" present_complete_copy)"
present_idle="$(positive_field "$completion" present_idle)"
present_idle_fence_triggers="$(
    positive_field "$completion" present_idle_fence_triggers
)"
native_max_render_msec="$(
    rendering_performance_field "$completion" native_max_render_msec
)" || fail "completion lacks native_max_render_msec"
native_max_upload_msec="$(
    rendering_performance_field "$completion" native_max_upload_msec
)" || fail "completion lacks native_max_upload_msec"
native_max_submit_to_page_flip_msec="$(
    rendering_performance_field "$completion" native_max_submit_to_page_flip_msec
)" || fail "completion lacks native_max_submit_to_page_flip_msec"
native_resources="$(
    grep -E '^sophia_live_native_resources schema=(5|6|7|8|9|10|11|12) status=complete ' "$SESSION_LOG" |
        tail -n 1 || true
)"
[[ -n "$native_resources" ]] || fail "missing native import-cache metrics"
import_cache_imports="$(positive_field "$native_resources" import_cache_imports)"
import_cache_hits="$(nonnegative_field "$native_resources" import_cache_hits)"
snapshot_captures="$(positive_field "$native_resources" snapshot_captures)"
snapshot_promotions="$(positive_field "$native_resources" snapshot_promotions)"
for assignment in \
    snapshot_rollbacks=0 \
    snapshot_live_entries=0 \
    snapshot_live_bytes=0 \
    import_cache_live_entries=0 \
    import_cache_descriptor_mismatches=0 \
    import_cache_capacity_rejections=0; do
    [[ " $native_resources " == *" $assignment "* ]] ||
        fail "native resource metrics do not contain $assignment"
done

# Either cursor path satisfies this gate.
#
# What it is for is that pointer motion does not perturb frame pacing, and
# that holds whichever way the cursor reaches its plane. The two assertions
# that used to sit here were about the legacy path's shape rather than about
# cadence: it required `path=legacy_ioctl` literally, and required
# `updates_primary_in_flight` to be strictly positive -- an ioctl moving a
# cursor while a flip is outstanding. On the atomic path that count is zero
# by construction, because the kernel serializes commits per CRTC and the
# cursor waits instead. Keeping them would have failed the atomic path for
# behaving correctly.
# The benchmark runs a standalone session; an older gate still demanded an
# external policy client, which a standalone
# session never reports. That made the gate unrunnable through its own
# benchmark script -- it could only ever pass against its fixture.
#
# What the gate is for is that pointer motion does not perturb frame pacing,
# and a window manager has no part in that. Either shape is accepted; a
# degraded or restarting WM is still refused above.
wm_policy="$(field "$completion" wm_policy)"
case "$wm_policy" in
external | disabled) ;;
*) fail "session reported an unexpected wm_policy: $wm_policy" ;;
esac

cursor="$(
    grep -E '^sophia_live_session_cursor schema=(5|6) path=(legacy_ioctl|atomic_plane) ' "$SESSION_LOG" |
        tail -n 1 || true
)"
[[ -n "$cursor" ]] || fail "missing hardware-cursor metrics"
cursor_path="$(field "$cursor" path)"
cursor_updates_primary_in_flight="$(
    nonnegative_field "$cursor" updates_primary_in_flight
)"
# The legacy path overlaps flips and the atomic path cannot; each is checked
# for its own shape rather than both for one.
if [[ "$cursor_path" == legacy_ioctl ]]; then
    ((cursor_updates_primary_in_flight > 0)) ||
        fail "the legacy cursor never overlapped a page flip, so pointer motion was not exercised"
else
    ((cursor_updates_primary_in_flight == 0)) ||
        fail "an atomic cursor committed while a flip was in flight"
fi
cursor_max_update_msec="$(nonnegative_field "$cursor" max_update_msec)"
cursor_hardware_failures="$(nonnegative_field "$cursor" hardware_failures)"
((cursor_max_update_msec <= 20)) ||
    fail "cursor updates exceeded the 20 ms steady-update budget"
((cursor_hardware_failures == 0)) || fail "hardware cursor update failed"

printf '%s\n' \
    "sophia_glxgears_performance schema=6 status=pass workload=glxgears-x11 role=compatibility_probe duration_seconds=$duration_seconds surface_width=$surface_width surface_height=$surface_height swap_interval=$swap_interval renderer_sha256=$renderer_sha256 output_pixels=$output_pixels client_samples=$client_samples client_mean_fps=$client_mean_fps present_samples=$timestamp_count present_fps=$present_fps p95_frame_msec=$p95_msec native_retirements=$native_retirements native_nonzero_exports=$native_nonzero_exports native_mixed_exports=$native_mixed_exports present_complete_copy=$present_complete_copy present_idle=$present_idle present_idle_fence_triggers=$present_idle_fence_triggers snapshot_captures=$snapshot_captures snapshot_promotions=$snapshot_promotions import_cache_imports=$import_cache_imports import_cache_hits=$import_cache_hits native_max_render_msec=$native_max_render_msec native_max_upload_msec=$native_max_upload_msec native_max_submit_to_page_flip_msec=$native_max_submit_to_page_flip_msec cursor_path=$cursor_path cursor_updates_primary_in_flight=$cursor_updates_primary_in_flight cursor_max_update_msec=$cursor_max_update_msec"
