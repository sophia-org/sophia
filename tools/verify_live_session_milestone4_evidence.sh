#!/usr/bin/env bash
set -euo pipefail

EVIDENCE_FILE="${1:-${SOPHIA_M4_GPU_EVIDENCE:-/tmp/sophia-m4-vkcube.log}}"

if [[ ! -s "$EVIDENCE_FILE" ]]; then
    echo "Milestone 4 GPU evidence is missing or empty: $EVIDENCE_FILE" >&2
    exit 1
fi
if grep -q '^Error:' "$EVIDENCE_FILE"; then
    echo "Milestone 4 GPU evidence contains a terminal error" >&2
    exit 1
fi

mapfile -t lines < <(grep -E '^sophia_live_session .*status=bounded_complete ' "$EVIDENCE_FILE" || true)
if (( ${#lines[@]} != 1 )); then
    echo "Milestone 4 GPU evidence expected exactly one completion line" >&2
    exit 1
fi

read -r -a parts <<<"${lines[0]}"
declare -A observed=()
for field in "${parts[@]:1}"; do
    [[ "$field" == *=* ]] || {
        echo "Milestone 4 GPU evidence has malformed field: $field" >&2
        exit 1
    }
    key="${field%%=*}"
    [[ -z "${observed[$key]+set}" ]] || {
        echo "Milestone 4 GPU evidence has duplicate field: $key" >&2
        exit 1
    }
    observed["$key"]="${field#*=}"
done

required=(
    schema status native_presentation native_submissions native_retirements
    native_submit_failures native_retire_failures native_callback_accepted
    native_callback_rejected native_callback_queue_saturated native_mixed_exports
    native_in_flight native_cleanup_pending cpu_layers present_complete_flip
    present_complete_skip present_idle present_idle_fence_triggers
    present_disconnect_failures present_live_sources present_live_fences
    present_live_transactions present_acquire_waits present_controlled_rejections
)
# Schema 16 separates Copy from Flip; both are successful X Present completions.
if [[ "${observed[schema]:-}" == "16" ]]; then
    required+=(present_complete_copy startup_ready_msec)
fi
for key in "${required[@]}"; do
    [[ -n "${observed[$key]+set}" ]] || {
        echo "Milestone 4 GPU evidence is missing field: $key" >&2
        exit 1
    }
done

[[ "${observed[schema]}" =~ ^(14|16|18)$ ]] || {
    echo "Milestone 4 GPU evidence requires a supported startup-proof schema" >&2
    exit 1
}
[[ "${observed[status]}" == "bounded_complete" ]]
[[ "${observed[native_presentation]}" == "enabled" ]]
[[ "${observed[native_in_flight]}" == "false" ]]
[[ "${observed[native_cleanup_pending]}" == "false" ]]

numeric=(
    native_submissions native_retirements native_submit_failures
    native_retire_failures native_callback_accepted native_callback_rejected
    native_callback_queue_saturated native_mixed_exports cpu_layers
    present_complete_flip present_complete_skip present_idle
    present_idle_fence_triggers present_disconnect_failures
    present_live_sources present_live_fences present_live_transactions
    present_acquire_waits present_controlled_rejections
)
if [[ "${observed[schema]}" == "16" ]]; then
    numeric+=(present_complete_copy startup_ready_msec)
fi
for key in "${numeric[@]}"; do
    [[ "${observed[$key]}" =~ ^[0-9]+$ ]] || {
        echo "Milestone 4 GPU evidence expected numeric $key" >&2
        exit 1
    }
done

positive=(
    native_submissions native_retirements native_callback_accepted
    native_mixed_exports cpu_layers present_complete_skip
    present_idle present_idle_fence_triggers present_acquire_waits
)
for key in "${positive[@]}"; do
    (( observed[$key] > 0 )) || {
        echo "Milestone 4 GPU evidence expected positive $key" >&2
        exit 1
    }
done

zero=(
    native_submit_failures native_retire_failures native_callback_rejected
    native_callback_queue_saturated present_disconnect_failures
    present_live_sources present_live_fences present_live_transactions
)
for key in "${zero[@]}"; do
    (( observed[$key] == 0 )) || {
        echo "Milestone 4 GPU evidence expected zero $key" >&2
        exit 1
    }
done

if (( observed[present_controlled_rejections] != 1 )); then
    echo "Milestone 4 GPU evidence expected exactly one controlled rejection" >&2
    exit 1
fi
successful=${observed[present_complete_flip]}
if [[ "${observed[schema]}" == "16" ]]; then
    successful=$((successful + observed[present_complete_copy]))
fi
if (( successful == 0 )); then
    echo "Milestone 4 GPU evidence requires a successful Present completion" >&2
    exit 1
fi
if (( observed[present_idle] != successful + observed[present_complete_skip] )); then
    echo "Milestone 4 GPU evidence has unmatched Complete/Idle lifecycles" >&2
    exit 1
fi

echo "Milestone 4 GPU evidence passed: $EVIDENCE_FILE"
