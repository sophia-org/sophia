#!/usr/bin/env bash
set -euo pipefail

# Terminal CPU-path throughput reporter. Reduces a bounded xterm standalone
# session log to one fail-closed sophia_terminal_performance schema=6 line.
# Unlike the vkcube/glxgears reporters (GPU DRI3 flip path), this asserts the
# software-Present (CPU) evidence: positive immutable patch-batch traffic,
# continuous post-readiness visual progress, bounded CPU compose time, and clean
# teardown.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=tools/lib/rendering_performance.sh
source "$ROOT_DIR/tools/lib/rendering_performance.sh"

STATE_HOME="${XDG_STATE_HOME:-${HOME}/.local/state}"
LOG_DIR="${SOPHIA_STANDALONE_LOG_DIR:-$STATE_HOME/sophia/standalone-session}"
SESSION_LOG="${1:-$LOG_DIR/session.log}"
COMPOSE_BUDGET_MSEC="${SOPHIA_TERMINAL_COMPOSE_BUDGET_MSEC:-25}"

fail() {
    echo "Sophia terminal performance report failed: $*" >&2
    exit 1
}

positive_field() {
    local line="$1" key="$2" value
    value="$(rendering_performance_field "$line" "$key")" ||
        fail "line lacks $key"
    [[ "$value" =~ ^[0-9]+$ ]] || fail "$key is not an integer"
    ((value > 0)) || fail "$key is not positive"
    printf '%s\n' "$value"
}

nonnegative_field() {
    local line="$1" key="$2" value
    value="$(rendering_performance_field "$line" "$key")" ||
        fail "line lacks $key"
    [[ "$value" =~ ^[0-9]+$ ]] || fail "$key is not a nonnegative integer"
    printf '%s\n' "$value"
}

[[ -s "$SESSION_LOG" ]] || fail "missing session log: $SESSION_LOG"
[[ "$COMPOSE_BUDGET_MSEC" =~ ^[1-9][0-9]*$ ]] ||
    fail "SOPHIA_TERMINAL_COMPOSE_BUDGET_MSEC must be a positive integer"

if grep -Eqi \
    '(^Error:|panicked at|admission_group_(invalid|overflowed)|mismatched.transaction|status=(failed|degraded)([[:space:]]|$))' \
    "$SESSION_LOG"; then
    fail "session contains an error, invalid admission group, or degraded status"
fi

benchmark="$(
    grep -E '^sophia_terminal_benchmark schema=2 workload=xterm-cpu ' "$SESSION_LOG" |
        tail -n 1 || true
)"
[[ -n "$benchmark" ]] || fail "missing terminal benchmark metadata"
client="$(
    grep -E '^sophia_xterm_client schema=2 status=complete ' "$SESSION_LOG" |
        tail -n 1 || true
)"
[[ -n "$client" ]] || fail "missing bounded xterm client completion"
completion="$(
    grep -E '^sophia_live_session schema=(16|17|18|19) status=bounded_complete ' "$SESSION_LOG" |
        tail -n 1 || true
)"
[[ -n "$completion" ]] || fail "missing bounded Sophia session completion"
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
    wm_degraded=false \
    present_live_sources=0 \
    present_live_fences=0 \
    present_live_transactions=0; do
    [[ " $completion " == *" $assignment "* ]] ||
        fail "completion does not contain $assignment"
done

# Benchmark metadata / client throughput.
duration_seconds="$(positive_field "$benchmark" duration_seconds)"
surface_width="$(positive_field "$benchmark" surface_width)"
surface_height="$(positive_field "$benchmark" surface_height)"
lines_per_iteration="$(positive_field "$benchmark" lines_per_iteration)"
interval_msec="$(positive_field "$benchmark" interval_msec)"
client_duration_seconds="$(positive_field "$client" duration_seconds)"
client_lines_per_iteration="$(positive_field "$client" lines_per_iteration)"
client_interval_msec="$(positive_field "$client" interval_msec)"
client_lines="$(positive_field "$client" lines)"
client_iterations="$(positive_field "$client" iterations)"
client_timed_exit="$(rendering_performance_field "$client" timed_exit)" ||
    fail "client completion lacks timed_exit"
[[ "$client_timed_exit" == true ]] || fail "xterm client did not complete its bounded window"
[[ "$client_duration_seconds" == "$duration_seconds" ]] ||
    fail "client duration does not match benchmark metadata"
[[ "$client_lines_per_iteration" == "$lines_per_iteration" ]] ||
    fail "client line batch does not match benchmark metadata"
[[ "$client_interval_msec" == "$interval_msec" ]] ||
    fail "client interval does not match benchmark metadata"
((client_lines == client_iterations * lines_per_iteration)) ||
    fail "client line total does not match iterations times line batch"

# Continuous progress is distinct from aggregate throughput. The previous
# startup-only failure had positive updates, compositions, and retirements, but
# all of them happened in the first 150ms before the terminal stopped changing.
# This compositor-owned record accounts every accepted post-readiness update and
# ties the latest non-superseded content to primary-plane retirement.
progress_count="$(grep -Ec '^sophia_live_cpu_visual_progress schema=(1|2|3) status=complete ' "$SESSION_LOG" || true)"
((progress_count == 1)) ||
    fail "expected exactly one CPU visual-progress record, found $progress_count"
progress="$(grep -E '^sophia_live_cpu_visual_progress schema=(1|2|3) status=complete ' "$SESSION_LOG")"
progress_schema="$(positive_field "$progress" schema)"
post_startup_updates="$(positive_field "$progress" post_startup_updates)"
post_startup_compositions="$(positive_field "$progress" compositions)"
primary_retirements="$(positive_field "$progress" primary_retirements)"
changed_primary_retirements="$(positive_field "$progress" changed_primary_retirements)"
presented_updates="$(positive_field "$progress" presented_updates)"
superseded_updates="$(nonnegative_field "$progress" superseded_updates)"
pending_updates="$(nonnegative_field "$progress" pending_updates)"
discarded_updates="$(nonnegative_field "$progress" discarded_updates)"
accounted_updates="$(positive_field "$progress" accounted_updates)"
observed_msec="$(positive_field "$progress" observed_msec)"
first_update_after_ready_msec="$(nonnegative_field "$progress" first_update_after_ready_msec)"
last_source_to_completion_msec="$(nonnegative_field "$progress" last_source_to_completion_msec)"
native_target_bindings=0
lifecycle_superseded_updates=0
if ((progress_schema >= 3)); then
    native_target_bindings="$(nonnegative_field "$progress" native_target_bindings)"
    lifecycle_superseded_updates="$(
        nonnegative_field "$progress" lifecycle_superseded_updates
    )"
fi
source_max_gap_msec="$(nonnegative_field "$progress" source_max_gap_msec)"
first_retirement_after_ready_msec="$(nonnegative_field "$progress" first_retirement_after_ready_msec)"
last_retirement_after_ready_msec="$(nonnegative_field "$progress" last_retirement_after_ready_msec)"
display_max_gap_msec="$(nonnegative_field "$progress" display_max_gap_msec)"
max_update_to_retirement_usec="$(nonnegative_field "$progress" max_update_to_retirement_usec)"
refresh_millihz="$(positive_field "$progress" refresh_millihz)"
if ((progress_schema >= 2)); then
    source_max_gap_usec="$(nonnegative_field "$progress" source_max_gap_usec)"
    display_max_gap_usec="$(nonnegative_field "$progress" display_max_gap_usec)"
else
    source_max_gap_usec=$((source_max_gap_msec * 1000))
    display_max_gap_usec=$((display_max_gap_msec * 1000))
fi

((changed_primary_retirements >= 3)) ||
    fail "fewer than three changed post-readiness primary retirements"
((primary_retirements >= changed_primary_retirements)) ||
    fail "changed primary retirements exceed all primary retirements"
((pending_updates == 0)) ||
    fail "post-readiness CPU updates remained pending after native drain"
((discarded_updates == 0)) ||
    fail "post-readiness CPU updates were discarded without supersession"
((accounted_updates == post_startup_updates)) ||
    fail "post-readiness CPU update accounting does not balance"
((presented_updates + superseded_updates + pending_updates == accounted_updates)) ||
    fail "presented/superseded/pending update settlement does not balance"
((first_update_after_ready_msec <= 1000)) ||
    fail "first CPU source progress arrived more than one second after readiness"
if ((progress_schema >= 3)); then
    ((native_target_bindings <= post_startup_compositions)) ||
        fail "native target bindings exceed queued logical compositions"
    ((presented_updates <= native_target_bindings)) ||
        fail "presented updates exceed exact native target bindings"
    ((lifecycle_superseded_updates <= superseded_updates)) ||
        fail "lifecycle supersessions exceed all superseded updates"
fi
((last_source_to_completion_msec <= 1000)) ||
    fail "CPU source stopped more than one second before session completion"
((first_retirement_after_ready_msec <= 1000)) ||
    fail "first changed primary retirement arrived more than one second after readiness"
((last_retirement_after_ready_msec <= observed_msec)) ||
    fail "last changed primary retirement is later than the observation window"
last_retirement_to_completion_msec=$((observed_msec - last_retirement_after_ready_msec))
((last_retirement_to_completion_msec <= 1000)) ||
    fail "changed primary retirement stopped more than one second before completion"

# refresh_millihz is hertz * 1000, so one refresh interval in microseconds is
# 1,000,000,000 / refresh_millihz. Round upward before granting two intervals
# plus one millisecond for timestamp quantization.
refresh_interval_usec=$(((1000000000 + refresh_millihz - 1) / refresh_millihz))
retirement_deadline_usec=$((2 * refresh_interval_usec + 1000))
display_gap_budget_usec=$retirement_deadline_usec
source_gap_budget_usec=$((3 * interval_msec * 1000))
if ((source_gap_budget_usec < retirement_deadline_usec)); then
    source_gap_budget_usec=$retirement_deadline_usec
fi
((source_max_gap_usec <= source_gap_budget_usec)) ||
    fail "CPU source progress exceeded its cadence budget"
((display_max_gap_usec <= display_gap_budget_usec)) ||
    fail "changed primary retirement exceeded its two-refresh cadence budget"
((max_update_to_retirement_usec <= retirement_deadline_usec)) ||
    fail "latest non-superseded CPU update missed the two-refresh retirement deadline"

# CPU software-Present evidence: the patch-batch path must have been exercised,
# not whole-pixmap replacement every present.
efficiency="$(
    grep -E '^sophia_live_rendering_efficiency schema=(1|2) status=complete ' "$SESSION_LOG" |
        tail -n 1 || true
)"
[[ -n "$efficiency" ]] || fail "missing rendering-efficiency evidence"
cpu_updates="$(positive_field "$efficiency" cpu_updates)"
cpu_patch_updates="$(positive_field "$efficiency" cpu_patch_updates)"
cpu_payload_bytes="$(positive_field "$efficiency" cpu_payload_bytes)"
cpu_patch_rects="$(positive_field "$efficiency" cpu_patch_rects)"
cpu_replacements="$(nonnegative_field "$efficiency" cpu_replacements)"
composition_target_reuses="$(nonnegative_field "$efficiency" composition_target_reuses)"
cpu_max_compose_msec="$(
    rendering_performance_field "$completion" cpu_max_compose_msec
)" || fail "completion lacks cpu_max_compose_msec"
[[ "$cpu_max_compose_msec" =~ ^[0-9]+$ ]] ||
    fail "cpu_max_compose_msec is not an integer"
((cpu_max_compose_msec <= COMPOSE_BUDGET_MSEC)) ||
    fail "CPU composition exceeded ${COMPOSE_BUDGET_MSEC}ms: ${cpu_max_compose_msec}ms"

# Copy-on-write backing, where schema 2 reports it. This is the workload that
# exercises it: xterm draws in software, so its pixels take the CPU presentation
# path that the shared backing changed. A GPU-backed client reports zeros here
# and proves nothing about it either way.
#
# The peaks are what "bounded" is a claim about -- a registry that ends empty
# having held a thousand buffers reads like one that never held four. The split
# count carries no threshold: it counts presentations still holding bytes when
# an update arrives, which is ordinary and workload-shaped, and is reported so a
# run that copies on every single update is visible rather than silent.
efficiency_schema="$(sed -n 's/^sophia_live_rendering_efficiency schema=\([0-9]*\) .*/\1/p' <<<"$efficiency")"
cpu_cow_splits=unknown
cpu_resident_buffers_peak=unknown
cpu_resident_bytes_peak=unknown
if [[ "$efficiency_schema" =~ ^[0-9]+$ ]] && ((efficiency_schema >= 2)); then
    cpu_cow_splits="$(nonnegative_field "$efficiency" cpu_cow_splits)"
    cpu_resident_buffers_peak="$(nonnegative_field "$efficiency" cpu_resident_buffers_peak)"
    cpu_resident_bytes_peak="$(nonnegative_field "$efficiency" cpu_resident_bytes_peak)"
    ((cpu_resident_buffers_peak > 0)) ||
        fail "the CPU registry never held a buffer, so the software path was not exercised"
    ((cpu_cow_splits <= cpu_patch_updates)) ||
        fail "more copies ($cpu_cow_splits) than patches ($cpu_patch_updates) that could have caused them"
fi

# Damage-driven repaint proof: at least one partial repaint, not a full frame
# on every present.
partial_repaints="$(
    grep -Ec 'sophia_live_output_repaint schema=1 .* mode=partial ' "$SESSION_LOG" || true
)"
full_repaints="$(
    grep -Ec 'sophia_live_output_repaint schema=1 .* mode=full ' "$SESSION_LOG" || true
)"
((partial_repaints > 0)) ||
    fail "no damage-driven partial repaint; the CPU path repainted full frames"

# Optional presentation cadence, if the session summarized it.
cadence="$(
    grep -E '^sophia_live_present_cadence schema=1 status=complete ' "$SESSION_LOG" |
        tail -n 1 || true
)"
present_samples=0
present_fps=none
p95_frame_msec=none
if [[ -n "$cadence" ]]; then
    present_samples="$(rendering_performance_field "$cadence" samples)" ||
        fail "cadence summary lacks samples"
    present_fps="$(rendering_performance_field "$cadence" mean_fps)" ||
        fail "cadence summary lacks mean_fps"
    p95_frame_msec="$(rendering_performance_field "$cadence" p95_frame_msec)" ||
        fail "cadence summary lacks p95_frame_msec"
fi

native_retirements="$(positive_field "$completion" native_retirements)"

printf '%s\n' \
    "sophia_terminal_performance schema=6 status=pass workload=xterm-cpu duration_seconds=$duration_seconds surface_width=$surface_width surface_height=$surface_height lines_per_iteration=$lines_per_iteration interval_msec=$interval_msec client_lines=$client_lines client_iterations=$client_iterations native_retirements=$native_retirements cpu_updates=$cpu_updates cpu_replacements=$cpu_replacements cpu_patch_updates=$cpu_patch_updates cpu_patch_rects=$cpu_patch_rects cpu_payload_bytes=$cpu_payload_bytes cpu_max_compose_msec=$cpu_max_compose_msec cpu_compose_budget_msec=$COMPOSE_BUDGET_MSEC composition_target_reuses=$composition_target_reuses partial_repaints=$partial_repaints full_repaints=$full_repaints post_startup_updates=$post_startup_updates post_startup_compositions=$post_startup_compositions native_target_bindings=$native_target_bindings changed_primary_retirements=$changed_primary_retirements presented_updates=$presented_updates superseded_updates=$superseded_updates lifecycle_superseded_updates=$lifecycle_superseded_updates source_max_gap_msec=$source_max_gap_msec source_max_gap_usec=$source_max_gap_usec source_gap_budget_usec=$source_gap_budget_usec first_retirement_after_ready_msec=$first_retirement_after_ready_msec display_max_gap_msec=$display_max_gap_msec display_max_gap_usec=$display_max_gap_usec display_gap_budget_usec=$display_gap_budget_usec last_retirement_to_completion_msec=$last_retirement_to_completion_msec max_update_to_retirement_usec=$max_update_to_retirement_usec retirement_deadline_usec=$retirement_deadline_usec present_samples=$present_samples present_fps=$present_fps p95_frame_msec=$p95_frame_msec cpu_cow_splits=$cpu_cow_splits cpu_resident_buffers_peak=$cpu_resident_buffers_peak cpu_resident_bytes_peak=$cpu_resident_bytes_peak"
