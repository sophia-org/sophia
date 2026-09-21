#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=tools/lib/rendering_performance.sh
source "$ROOT_DIR/tools/lib/rendering_performance.sh"

STATE_HOME="${XDG_STATE_HOME:-${HOME}/.local/state}"
LOG_DIR="${SOPHIA_STANDALONE_LOG_DIR:-$STATE_HOME/sophia/standalone-session}"
SESSION_LOG="${1:-$LOG_DIR/session.log}"
BASELINE_FPS="${SOPHIA_RENDER_BASELINE_FPS:-}"
BASELINE_P95_MSEC="${SOPHIA_RENDER_BASELINE_P95_MSEC:-}"
MIN_BASELINE_RATIO="${SOPHIA_RENDER_MIN_BASELINE_RATIO:-0.90}"

fail() {
    echo "Sophia rendering performance report failed: $*" >&2
    exit 1
}

[[ -s "$SESSION_LOG" ]] || fail "missing session log: $SESSION_LOG"

completion="$(
    grep -E '^sophia_live_session schema=(15|16|18) status=bounded_complete ' "$SESSION_LOG" |
        tail -n 1
)"
[[ -n "$completion" ]] || fail "missing bounded session completion"
efficiency="$(
    grep -E '^sophia_live_rendering_efficiency schema=(1|2) status=complete ' "$SESSION_LOG" |
        tail -n 1
)"
[[ -n "$efficiency" ]] || fail "missing rendering-efficiency evidence"
benchmark="$(
    grep -E '^sophia_rendering_benchmark schema=1 ' "$SESSION_LOG" |
        tail -n 1 || true
)"
gpu_line="$(
    grep -E '^Selected GPU [0-9]+: ' "$SESSION_LOG" |
        head -n 1 || true
)"
[[ -n "$gpu_line" ]] || fail "missing selected Vulkan provider"
gpu_identity="${gpu_line#*: }"
gpu_sha256="$(printf '%s' "$gpu_identity" | sha256sum | awk '{print $1}')"
output_repaint="$(
    grep -E '^.*sophia_live_output_repaint schema=1 status=[^ ]+ output=1 mode=full ' \
        "$SESSION_LOG" |
        head -n 1 || true
)"
output_pixels="$(rendering_performance_field "$output_repaint" pixels 2>/dev/null || true)"

TIMESTAMPS_FILE="$(mktemp)"
INTERVALS_FILE="$(mktemp)"
trap 'rm -f "$TIMESTAMPS_FILE" "$INTERVALS_FILE" "$INTERVALS_FILE.fps"' EXIT

awk '
    /^sophia_live_session_present_feedback schema=1 kind=complete / &&
        / routed=true / && / mode=Flip / {
        for (field_index = 1; field_index <= NF; field_index++) {
            if ($field_index ~ /^ust=[0-9]+$/) {
                split($field_index, pair, "=")
                print pair[2]
            }
        }
    }
' "$SESSION_LOG" >"$TIMESTAMPS_FILE"

if ! read -r timestamp_count fps p95_msec < <(
    rendering_performance_cadence "$TIMESTAMPS_FILE" "$INTERVALS_FILE"
); then
    fail "need at least three advancing routed Flip timestamps"
fi

native_retirements="$(rendering_performance_field "$completion" native_retirements)" ||
    fail "completion lacks native_retirements"
native_max_upload_msec="$(rendering_performance_field "$completion" native_max_upload_msec)" ||
    fail "completion lacks native_max_upload_msec"
cpu_max_compose_msec="$(rendering_performance_field "$completion" cpu_max_compose_msec)" ||
    fail "completion lacks cpu_max_compose_msec"
cpu_replacements="$(rendering_performance_field "$efficiency" cpu_replacements)" ||
    fail "efficiency evidence lacks cpu_replacements"
cpu_patch_updates="$(rendering_performance_field "$efficiency" cpu_patch_updates)" ||
    fail "efficiency evidence lacks cpu_patch_updates"
cpu_patch_rects="$(rendering_performance_field "$efficiency" cpu_patch_rects)" ||
    fail "efficiency evidence lacks cpu_patch_rects"
cpu_payload_bytes="$(rendering_performance_field "$efficiency" cpu_payload_bytes)" ||
    fail "efficiency evidence lacks cpu_payload_bytes"
target_reuses="$(rendering_performance_field "$efficiency" composition_target_reuses)" ||
    fail "efficiency evidence lacks composition_target_reuses"
requested_frames="$(
    rendering_performance_field "$benchmark" requested_frames 2>/dev/null || true
)"
surface_width="$(
    rendering_performance_field "$benchmark" surface_width 2>/dev/null || true
)"
surface_height="$(
    rendering_performance_field "$benchmark" surface_height 2>/dev/null || true
)"
vulkan_present_mode="$(
    rendering_performance_field "$benchmark" vulkan_present_mode 2>/dev/null || true
)"

status=pass
if [[ -n "$BASELINE_FPS" ]]; then
    awk -v observed="$fps" -v baseline="$BASELINE_FPS" -v ratio="$MIN_BASELINE_RATIO" \
        'BEGIN { exit !(observed >= baseline * ratio) }' ||
        status=below_baseline
fi
if [[ -n "$BASELINE_P95_MSEC" ]]; then
    awk -v observed="$p95_msec" -v baseline="$BASELINE_P95_MSEC" \
        -v ratio="$MIN_BASELINE_RATIO" \
        'BEGIN { exit !(observed <= baseline / ratio) }' ||
        status=below_baseline
fi

printf '%s\n' \
    "sophia_rendering_performance schema=2 status=$status workload=vkcube-xcb requested_frames=${requested_frames:-unknown} surface_width=${surface_width:-unknown} surface_height=${surface_height:-unknown} vulkan_present_mode=${vulkan_present_mode:-unknown} x_present_path=Flip gpu_sha256=$gpu_sha256 output_pixels=${output_pixels:-unknown} present_samples=$timestamp_count flip_samples=$timestamp_count fps=$fps p95_frame_msec=$p95_msec native_retirements=$native_retirements cpu_max_compose_msec=$cpu_max_compose_msec native_max_upload_msec=$native_max_upload_msec cpu_replacements=$cpu_replacements cpu_patch_updates=$cpu_patch_updates cpu_patch_rects=$cpu_patch_rects cpu_payload_bytes=$cpu_payload_bytes composition_target_reuses=$target_reuses baseline_fps=${BASELINE_FPS:-none} baseline_p95_msec=${BASELINE_P95_MSEC:-none} min_baseline_ratio=$MIN_BASELINE_RATIO"

[[ "$status" == pass ]] || fail "observed cadence is below the configured baseline gate"
