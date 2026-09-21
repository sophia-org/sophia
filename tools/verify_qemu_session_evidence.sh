#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
EVIDENCE_FILE="${1:-${SOPHIA_QEMU_EVIDENCE:-/tmp/sophia-qemu-session.log}}"

"$ROOT_DIR/tools/verify_live_session_persistent_evidence.sh" "$EVIDENCE_FILE"

if [[ "$(grep -c '^sophia_qemu_session schema=3 status=starting isolation=headless display_sink=vnc-unix control=qmp-unix host_drm=none host_vt=none guest_network=none storage=none gpu=virtio-gpu gpu_devices=2 gpu_heads=2 keyboard=virtio mouse=virtio ticks=300$' "$EVIDENCE_FILE" || true)" -ne 1 ]]; then
    echo "QEMU evidence is missing the isolated start marker" >&2
    exit 1
fi
if [[ "$(grep -c '^sophia_qemu_guest schema=1 status=complete ticks=300$' "$EVIDENCE_FILE" || true)" -ne 1 ]]; then
    echo "QEMU evidence is missing the 300-tick guest completion marker" >&2
    exit 1
fi
if [[ "$(grep -c '^sophia_qemu_session schema=3 status=complete qemu_exit=0$' "$EVIDENCE_FILE" || true)" -ne 1 ]]; then
    echo "QEMU evidence is missing the clean host completion marker" >&2
    exit 1
fi
if [[ "$(grep -c '^sophia_qemu_topology schema=1 status=observed requested_heads=2 connectors=2 connected=2$' "$EVIDENCE_FILE" || true)" -ne 1 ]]; then
    echo "QEMU evidence is missing two connected virtual outputs" >&2
    exit 1
fi
if [[ "$(grep -c '^sophia_live_outputs schema=2 status=ready discovered=2 presentation=2 native_owned=2 multi_output_scanout=enabled layout=extended_horizontal$' "$EVIDENCE_FILE" || true)" -ne 1 ]]; then
    echo "QEMU evidence is missing complete two-output native ownership" >&2
    exit 1
fi
mapfile -t output_lines < <(grep '^sophia_live_output schema=1 status=complete ' "$EVIDENCE_FILE" || true)
if [[ "${#output_lines[@]}" -ne 2 ]]; then
    echo "QEMU evidence must contain exactly two per-output completion records" >&2
    exit 1
fi
# This gate's whole claim is two independent logical outputs, and the record is
# emitted once per head rather than once per output. A mirror group would
# therefore produce two records that both name output 1 -- structurally two
# records, but one output. Counting distinct identities is what separates the
# two cases; identical content no longer can, because two independent outputs
# may legitimately present the same pixels.
declare -A output_ids=()
total_retirements=0
total_callbacks=0
total_nonzero_exports=0
for output_line in "${output_lines[@]}"; do
    output_id="$(sed -n 's/.* output=\([0-9][0-9]*\).*/\1/p' <<< "$output_line")"
    if [[ -z "$output_id" ]]; then
        echo "QEMU output evidence has no numeric output identity: $output_line" >&2
        exit 1
    fi
    output_ids[$output_id]=1
    submissions="$(sed -n 's/.* submissions=\([0-9][0-9]*\).*/\1/p' <<< "$output_line")"
    if [[ -z "$submissions" ]] || (( submissions == 0 )); then
        echo "QEMU output evidence has no submissions: $output_line" >&2
        exit 1
    fi
    # Exported pixels are counted across the desktop rather than demanded of
    # every output. An output holding no windows composes an all-black frame,
    # which is the correct picture for it and reports zero nonzero pixels; the
    # session still has to have rendered something somewhere.
    nonzero_exports="$(sed -n 's/.* nonzero_exports=\([0-9][0-9]*\).*/\1/p' <<< "$output_line")"
    if [[ -z "$nonzero_exports" ]]; then
        echo "QEMU output evidence is missing its nonzero export counter: $output_line" >&2
        exit 1
    fi
    total_nonzero_exports=$((total_nonzero_exports + nonzero_exports))
    retirements="$(sed -n 's/.* retirements=\([0-9][0-9]*\).*/\1/p' <<< "$output_line")"
    callbacks="$(sed -n 's/.* callbacks=\([0-9][0-9]*\).*/\1/p' <<< "$output_line")"
    if [[ -z "$retirements" || -z "$callbacks" ]]; then
        echo "QEMU output evidence is missing retirement counters: $output_line" >&2
        exit 1
    fi
    if [[ "${SOPHIA_QEMU_REQUIRE_TWO_XTERM:-0}" == "1" ]] \
        && (( retirements == 0 || callbacks == 0 )); then
        echo "QEMU two-xterm output evidence has no asynchronous retirement: $output_line" >&2
        exit 1
    fi
    total_retirements=$((total_retirements + retirements))
    total_callbacks=$((total_callbacks + callbacks))
    checksum="$(sed -n 's/.* checksum=\([0-9][0-9]*\) .*/\1/p' <<< "$output_line")"
    if [[ -z "$checksum" ]]; then
        echo "QEMU output evidence is missing its logical-content checksum" >&2
        exit 1
    fi
done
if (( ${#output_ids[@]} != 2 )); then
    echo "QEMU evidence names ${#output_ids[@]} distinct outputs; two required" >&2
    exit 1
fi
if (( total_retirements == 0 || total_callbacks == 0 )); then
    echo "QEMU evidence has no asynchronous page-flip retirement" >&2
    exit 1
fi
if (( total_nonzero_exports == 0 )); then
    echo "QEMU evidence has no nonzero exports on any output" >&2
    exit 1
fi
# How many renderer threads the session ran. Only schema 10 reports it, and
# only a run that declares what it expects asserts it: the count is the whole
# difference between outputs sharing a thread and outputs that merely could.
expected_workers="${SOPHIA_QEMU_EXPECT_RENDERER_WORKERS:-}"
if [[ -n "$expected_workers" ]]; then
    if ! [[ "$expected_workers" =~ ^[1-9][0-9]*$ ]]; then
        echo "SOPHIA_QEMU_EXPECT_RENDERER_WORKERS must be a positive integer" >&2
        exit 1
    fi
    resources_line="$(grep -E '^sophia_live_native_resources schema=[0-9]+ status=complete ' \
        "$EVIDENCE_FILE" || true)"
    if [[ -z "$resources_line" ]]; then
        echo "renderer-thread expectation needs schema-10 or newer resource evidence" >&2
        exit 1
    fi
    observed_workers="$(sed -n 's/.* renderer_workers=\([0-9][0-9]*\).*/\1/p' \
        <<< "$resources_line")"
    # renderer_workers arrived at schema 10. Evidence that predates it cannot
    # answer the question this expectation asks, and must say so rather than
    # comparing an empty string against the expected count.
    if [[ -z "$observed_workers" ]]; then
        echo "resource evidence carries no renderer_workers field" >&2
        exit 1
    fi
    if [[ "$observed_workers" != "$expected_workers" ]]; then
        echo "session ran $observed_workers renderer threads, expected $expected_workers" >&2
        exit 1
    fi
    misroutes="$(sed -n 's/.* worker_result_misroutes=\([0-9][0-9]*\).*/\1/p' \
        <<< "$resources_line")"
    if (( misroutes != 0 )); then
        echo "$misroutes renderer results reached an output that did not request them" >&2
        exit 1
    fi
fi
if [[ "$(grep -c '^sophia_live_vsync schema=1 status=complete outputs=2 overlap_rejections=0 phase_rejections=0 policy=page_flip_paced$' "$EVIDENCE_FILE" || true)" -ne 1 ]]; then
    echo "QEMU evidence is missing the per-output fixed-refresh vsync gate" >&2
    exit 1
fi
if grep -q '^sophia_qemu_.* status=failed' "$EVIDENCE_FILE"; then
    echo "QEMU evidence contains a failure marker" >&2
    exit 1
fi
if [[ "$(grep -c '^sophia_live_session_input schema=1 status=ready source=physical text=sophia$' "$EVIDENCE_FILE" || true)" -ne 1 ]]; then
    echo "QEMU evidence is missing physical-input readiness" >&2
    exit 1
fi
if [[ "$(grep -c '^sophia_qemu_input schema=1 status=sent source=qmp device=virtio-keyboard text=sophia events=14$' "$EVIDENCE_FILE" || true)" -ne 1 ]]; then
    echo "QEMU evidence is missing QMP keyboard delivery" >&2
    exit 1
fi
if [[ "$(grep -c '^sophia_live_session_input schema=2 status=complete source=physical text=sophia expected_events=14 matched_events=14 pixel_change=true$' "$EVIDENCE_FILE" || true)" -ne 1 ]]; then
    echo "QEMU evidence is missing the exact physical text completion record" >&2
    exit 1
fi
latency_line="$(grep '^sophia_live_input_latency schema=1 status=complete source=libinput_to_kernel_page_flip ' "$EVIDENCE_FILE" || true)"
if [[ "$(wc -l <<< "$latency_line")" -ne 1 ]]; then
    echo "QEMU evidence is missing the libinput-to-kernel-page-flip latency record" >&2
    exit 1
fi
for field in queue_dwell_msec dwell_to_submit_msec submit_to_page_flip_msec full_chain_msec; do
    value="$(sed -n "s/.* ${field}=\([0-9][0-9]*\).*/\1/p" <<< "$latency_line")"
    if [[ -z "$value" ]]; then
        echo "QEMU latency evidence is missing numeric $field" >&2
        exit 1
    fi
    printf -v "$field" '%s' "$value"
done
stage_total_msec=$((queue_dwell_msec + dwell_to_submit_msec + submit_to_page_flip_msec))
# The absolute bound is a miscorrelation guard, not a latency claim -- this
# guest renders in software under TCG, and the honest correlation includes
# the render the input actually waited on, which the previous 100 ms bound
# predated: it was calibrated against a correlation that accepted a page
# flip carrying a composition older than the press. Physical latency has its
# own gate with real budgets.
if (( stage_total_msec > full_chain_msec \
    || full_chain_msec - stage_total_msec > 2 \
    || full_chain_msec > 400 )); then
    echo "QEMU latency evidence has inconsistent stages or exceeds 400 ms" >&2
    exit 1
fi
clock_line="$(grep '^sophia_live_page_flip_clock schema=1 status=complete source=kernel_monotonic ' "$EVIDENCE_FILE" || true)"
if [[ "$(wc -l <<< "$clock_line")" -ne 1 ]] \
    || ! [[ "$clock_line" =~ timestamps=([1-9][0-9]*)\ fallbacks=0\ pending=0$ ]]; then
    echo "QEMU evidence lacks complete kernel page-flip timestamp coverage" >&2
    exit 1
fi
if [[ "$(grep -c '^sophia_live_session_pointer schema=1 status=ready source=physical action=select$' "$EVIDENCE_FILE" || true)" -ne 1 ]]; then
    echo "QEMU evidence is missing physical-pointer readiness" >&2
    exit 1
fi
if [[ "$(grep -c '^sophia_qemu_pointer schema=1 status=sent source=qmp device=virtio-mouse action=select commands=5$' "$EVIDENCE_FILE" || true)" -ne 1 ]]; then
    echo "QEMU evidence is missing QMP pointer delivery" >&2
    exit 1
fi

completion_line="$(grep -E '^sophia_live_session .*status=bounded_complete ' "$EVIDENCE_FILE")"
completion_schema="$(sed -n 's/.* schema=\([0-9][0-9]*\) .*/\1/p' <<< "$completion_line")"
if [[ ! "$completion_schema" =~ ^(10|11|14|15|16|18)$ ]]; then
    echo "QEMU evidence did not use the latency/resource schema" >&2
    exit 1
fi
if [[ ! " $completion_line " =~ " session_ticks=300 " ]]; then
    echo "QEMU evidence did not complete exactly 300 session ticks" >&2
    exit 1
fi
if [[ ! " $completion_line " =~ " physical_input=enabled " ]]; then
    echo "QEMU evidence did not open the virtual input path" >&2
    exit 1
fi
if [[ ! " $completion_line " =~ " injected_input=false " ]]; then
    echo "QEMU evidence used the internal X11 injection path" >&2
    exit 1
fi
# TWO TERMINALS MEANS TWO SURFACES STANDING TOGETHER, AND THAT IS A PEAK.
# This asked `cpu_layers >= 2` off the completion line for a long time. That
# field is one composition's layer count, sampled at the end, and a session
# that drains its clients correctly ends with nothing composed -- it read 2
# only because the last passing run (2026-07-18) has no quiescence records at
# all and ended with its clients still alive. It has read 0 on every healthy
# run since.
#
# `cpu_max_layers` is not the replacement, tempting as the name is: a
# direct-scanout copy reports one layer however many surfaces exist, and a
# damage-scoped frame counts only what that frame touched, so its peak says
# composition happened and nothing about how many windows were up.
# `runtime_max_surfaces` is the most surfaces ever committed at once, which is
# exactly the claim. Evidence older than schema 18 does not carry it and keeps
# the old reading, which is the only thing that field can support there.
if [[ "${SOPHIA_QEMU_REQUIRE_TWO_XTERM:-0}" == "1" ]] \
    && [[ "$completion_schema" =~ ^(18|19)$ ]]; then
    max_surfaces="$(sed -n 's/.* runtime_max_surfaces=\([0-9][0-9]*\) .*/\1/p' <<< "$completion_line")"
    if [[ ! "$max_surfaces" =~ ^[0-9]+$ ]] || (( max_surfaces < 2 )); then
        echo "QEMU two-xterm evidence never had two surfaces committed at once" >&2
        exit 1
    fi
fi
if [[ "${SOPHIA_QEMU_REQUIRE_TWO_XTERM:-0}" == "1" ]] \
    && [[ ! "$completion_schema" =~ ^(18|19)$ ]]; then
    cpu_layers="$(sed -n 's/.* cpu_layers=\([0-9][0-9]*\) .*/\1/p' <<< "$completion_line")"
    if [[ ! "$cpu_layers" =~ ^[0-9]+$ ]] || (( cpu_layers < 2 )); then
        echo "QEMU two-xterm evidence did not compose two terminal layers" >&2
        exit 1
    fi
    if ! grep -Eq '^sophia_live_session schema=7 status=running .* secondary_terminal=enabled ' "$EVIDENCE_FILE"; then
        echo "QEMU two-xterm evidence did not enable the secondary terminal" >&2
        exit 1
    fi
fi
physical_keys="$(sed -n 's/.* physical_keys_routed=\([0-9][0-9]*\) .*/\1/p' <<< "$completion_line")"
if [[ -z "$physical_keys" ]] || (( physical_keys != 14 )); then
    echo "QEMU evidence did not route exactly 14 virtio keyboard events" >&2
    exit 1
fi
if [[ ! " $completion_line " =~ " pointer_proof=enabled " ]] || [[ ! " $completion_line " =~ " pointer_pixel_change=true " ]]; then
    echo "QEMU evidence did not prove visible pointer input" >&2
    exit 1
fi
physical_pointer="$(sed -n 's/.* physical_pointer_routed=\([0-9][0-9]*\) .*/\1/p' <<< "$completion_line")"
if [[ -z "$physical_pointer" ]] || (( physical_pointer == 0 )); then
    echo "QEMU evidence has no routed virtio mouse events" >&2
    exit 1
fi

for field in input_presented_latency_msec cpu_max_compose_msec \
    native_max_submit_to_page_flip_msec native_max_upload_msec \
    native_target_creations native_target_recreations native_pipeline_creations \
    native_frame_uploads; do
    value="$(sed -n "s/.* ${field}=\([0-9][0-9]*\).*/\1/p" <<< "$completion_line")"
    if [[ -z "$value" ]]; then
        echo "QEMU evidence is missing numeric $field" >&2
        exit 1
    fi
    printf -v "$field" '%s' "$value"
done
native_frame_surface_creations=0
if [[ " $completion_line " =~ " schema=15 " ]] \
    || [[ " $completion_line " =~ " schema=16 " ]]; then
    native_frame_surface_creations="$(
        sed -n 's/.* native_frame_surface_creations=\([0-9][0-9]*\).*/\1/p' \
            <<<"$completion_line"
    )"
    if [[ -z "$native_frame_surface_creations" ]]; then
        echo "QEMU evidence is missing numeric native_frame_surface_creations" >&2
        exit 1
    fi
fi
# The presented-latency bound matches the miscorrelation guard above and for
# the same reason: the honest correlation includes the software render the
# input waited on, which the old 100 ms bound predated. The per-stage bounds
# keep their own meanings.
if (( input_presented_latency_msec > 400 || cpu_max_compose_msec > 25 \
    || native_max_submit_to_page_flip_msec > 100 || native_max_upload_msec > 100 )); then
    echo "QEMU evidence exceeded its input/rendering latency budget" >&2
    exit 1
fi
# Whole-frame CPU uploads belong to the retired queue_frame path: per-head
# composition renders into each head's own target, so native_frame_uploads is
# legitimately zero here and in both promoted native archives. What still has
# to hold is that targets were not rebuilt behind the session's back.
if (( native_target_recreations != 0 )); then
    echo "QEMU evidence did not preserve bounded native upload resources" >&2
    exit 1
fi
# Direct write creates neither resource; the persistent-GL path creates one
# pipeline per target, however many targets a session's heads and frame slots
# ask for. Pinning that count to one-target-per-output predates both per-head
# composition and the three-slot pool, so consistency is what is checked here.
if (( native_target_creations != native_pipeline_creations )); then
    echo "QEMU evidence has inconsistent direct-write/persistent-GL resource counters" >&2
    exit 1
fi
if { [[ " $completion_line " =~ " schema=15 " ]] \
    || [[ " $completion_line " =~ " schema=16 " ]]; } \
    && (( native_frame_surface_creations < native_target_creations )); then
    echo "QEMU evidence has fewer frame surfaces than renderer epochs" >&2
    exit 1
fi

echo "QEMU dual-output native presentation/QMP-input 300-tick evidence passed: $EVIDENCE_FILE"
