#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
FIXTURE_DIR="$ROOT_DIR/tools/fixtures"
TEMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TEMP_DIR"' EXIT

expect_pass() {
    local verifier="$1"
    local fixture="$2"

    "$ROOT_DIR/$verifier" "$FIXTURE_DIR/$fixture" >/dev/null
}

expect_fail() {
    local verifier="$1"
    local fixture="$2"

    if "$ROOT_DIR/$verifier" "$FIXTURE_DIR/$fixture" >/dev/null 2>&1; then
        echo "verifier unexpectedly accepted fixture: $fixture" >&2
        exit 1
    fi
}

# A refusal that fires for the wrong reason proves nothing about the clause
# under test, so these controls name the message they must produce.
expect_fail_reason() {
    local verifier="$1"
    local fixture="$2"
    local reason="$3"
    local output

    if output="$("$ROOT_DIR/$verifier" "$FIXTURE_DIR/$fixture" 2>&1 >/dev/null)"; then
        echo "verifier unexpectedly accepted fixture: $fixture" >&2
        exit 1
    fi
    if [[ "$output" != *"$reason"* ]]; then
        echo "fixture $fixture was refused for the wrong reason: $output" >&2
        exit 1
    fi
}

expect_pass tools/verify_atomic_scanout_preflight.sh atomic_scanout_preflight_pass.log
expect_fail tools/verify_atomic_scanout_preflight.sh atomic_scanout_preflight_unavailable.log
expect_fail tools/verify_atomic_scanout_preflight.sh atomic_scanout_preflight_impossible_counts.log
expect_fail tools/verify_atomic_scanout_preflight.sh atomic_scanout_preflight_unknown_native_field.log
expect_fail tools/verify_atomic_scanout_preflight.sh atomic_scanout_preflight_duplicate_field.log
expect_fail tools/verify_atomic_scanout_preflight.sh atomic_scanout_preflight_malformed_field.log
expect_fail tools/verify_atomic_scanout_preflight.sh atomic_scanout_preflight_multiple_lines.log

expect_pass tools/verify_atomic_scanout_evidence.sh atomic_scanout_evidence_pass.log
expect_pass tools/verify_atomic_scanout_evidence.sh atomic_scanout_evidence_pass_modifiers.log
expect_fail tools/verify_atomic_scanout_evidence.sh atomic_scanout_evidence_missing_rendered_context.log
expect_fail tools/verify_atomic_scanout_evidence.sh atomic_scanout_evidence_missing_scanout_buffer.log
expect_fail tools/verify_atomic_scanout_evidence.sh atomic_scanout_evidence_missing_steady_phase.log
expect_fail tools/verify_atomic_scanout_evidence.sh atomic_scanout_evidence_wrong_steady_scope.log
expect_fail tools/verify_atomic_scanout_evidence.sh atomic_scanout_evidence_unknown_native_field.log
expect_fail tools/verify_atomic_scanout_evidence.sh atomic_scanout_evidence_duplicate_field.log
expect_fail tools/verify_atomic_scanout_evidence.sh atomic_scanout_evidence_malformed_field.log
expect_fail tools/verify_atomic_scanout_evidence.sh atomic_scanout_evidence_waiting_retire.log
expect_fail tools/verify_atomic_scanout_evidence.sh atomic_scanout_evidence_cleanup_pending.log
expect_fail tools/verify_atomic_scanout_evidence.sh atomic_scanout_evidence_test_only_commit.log
expect_fail tools/verify_atomic_scanout_evidence.sh atomic_scanout_evidence_blocking_commit.log
expect_fail tools/verify_atomic_scanout_evidence.sh atomic_scanout_evidence_missing_page_flip_event_flag.log
expect_fail tools/verify_atomic_scanout_evidence.sh atomic_scanout_evidence_smoke_child_timeout.log

expect_pass tools/verify_runtime_rendered_scanout_evidence.sh runtime_rendered_scanout_evidence_pass.log
expect_pass tools/verify_runtime_rendered_scanout_evidence.sh runtime_rendered_scanout_evidence_pass_modifiers.log
expect_fail tools/verify_runtime_rendered_scanout_evidence.sh runtime_rendered_scanout_evidence_missing_retire.log
expect_fail tools/verify_runtime_rendered_scanout_evidence.sh runtime_rendered_scanout_evidence_cleanup_debt.log
expect_fail tools/verify_runtime_rendered_scanout_evidence.sh runtime_rendered_scanout_evidence_cleanup_retry.log
expect_fail tools/verify_runtime_rendered_scanout_evidence.sh runtime_rendered_scanout_evidence_unknown_field.log
expect_fail tools/verify_runtime_rendered_scanout_evidence.sh runtime_rendered_scanout_evidence_duplicate_field.log
expect_fail tools/verify_runtime_rendered_scanout_evidence.sh runtime_rendered_scanout_evidence_malformed_field.log
expect_fail tools/verify_runtime_rendered_scanout_evidence.sh runtime_rendered_scanout_evidence_failure.log

expect_pass tools/verify_live_session_content_evidence.sh live_session_content_evidence_pass.log
expect_fail tools/verify_live_session_content_evidence.sh live_session_content_evidence_checksum_mismatch.log

expect_pass tools/verify_live_session_persistent_evidence.sh live_session_persistent_evidence_pass.log
expect_pass tools/verify_live_session_persistent_evidence.sh live_session_persistent_evidence_physical_pass.log
expect_pass tools/verify_live_session_persistent_evidence.sh live_session_persistent_evidence_wm_pass.log
expect_pass tools/verify_live_session_persistent_evidence.sh live_session_persistent_evidence_v8_pass.log
expect_fail tools/verify_live_session_persistent_evidence.sh live_session_persistent_evidence_cleanup_debt.log
expect_fail tools/verify_live_session_persistent_evidence.sh live_session_persistent_evidence_physical_mismatch.log
expect_fail tools/verify_live_session_persistent_evidence.sh live_session_persistent_evidence_physical_missing.log
expect_fail tools/verify_live_session_persistent_evidence.sh live_session_persistent_evidence_post_completion_error.log

expect_pass tools/verify_live_session_two_xterm_evidence.sh live_session_two_xterm_evidence_pass.log
expect_fail tools/verify_live_session_two_xterm_evidence.sh live_session_two_xterm_evidence_slow_startup.log
expect_fail tools/verify_live_session_two_xterm_evidence.sh live_session_two_xterm_evidence_slow_compose.log

cp "$FIXTURE_DIR/live_session_two_xterm_evidence_pass.log" "$TEMP_DIR/classic.log"
sed 's/namespace_profile=classic_shared/namespace_profile=confined/g' \
    "$FIXTURE_DIR/live_session_two_xterm_evidence_pass.log" > "$TEMP_DIR/confined.log"
"$ROOT_DIR/tools/verify_live_session_milestone3_evidence.sh" \
    "$TEMP_DIR/classic.log" "$TEMP_DIR/confined.log" >/dev/null
if "$ROOT_DIR/tools/verify_live_session_milestone3_evidence.sh" \
    "$TEMP_DIR/classic.log" "$TEMP_DIR/classic.log" >/dev/null 2>&1; then
    echo "Milestone 3 verifier accepted classic evidence as confined evidence" >&2
    exit 1
fi
sed 's/namespace_request_capabilities=0/namespace_request_capabilities=1/' \
    "$TEMP_DIR/confined.log" > "$TEMP_DIR/confined-capability.log"
if "$ROOT_DIR/tools/verify_live_session_milestone3_evidence.sh" \
    "$TEMP_DIR/classic.log" "$TEMP_DIR/confined-capability.log" >/dev/null 2>&1; then
    echo "Milestone 3 verifier accepted a capability-bearing confined namespace" >&2
    exit 1
fi

expect_pass tools/verify_qemu_session_evidence.sh qemu_session_evidence_pass.log
sed -e "s/native_target_creations=2/native_target_creations=0/" \
    -e "s/native_pipeline_creations=2/native_pipeline_creations=0/" \
    "$FIXTURE_DIR/qemu_session_evidence_pass.log" > "$TEMP_DIR/qemu-direct-write.log"
"$ROOT_DIR/tools/verify_qemu_session_evidence.sh" \
    "$TEMP_DIR/qemu-direct-write.log" > /dev/null
sed "s/native_target_creations=0/native_target_creations=1/" \
    "$TEMP_DIR/qemu-direct-write.log" > "$TEMP_DIR/qemu-inconsistent-resources.log"
if "$ROOT_DIR/tools/verify_qemu_session_evidence.sh" \
    "$TEMP_DIR/qemu-inconsistent-resources.log" > /dev/null 2>&1; then
    echo "QEMU verifier accepted inconsistent direct-write resource evidence" >&2
    exit 1
fi
expect_fail tools/verify_qemu_session_evidence.sh qemu_session_evidence_wrong_ticks.log
expect_fail tools/verify_qemu_session_evidence.sh qemu_session_evidence_internal_input.log
expect_fail tools/verify_qemu_session_evidence.sh qemu_session_evidence_no_pointer_pixels.log
expect_fail tools/verify_qemu_session_evidence.sh qemu_session_evidence_one_connected_output.log
expect_fail tools/verify_qemu_session_evidence.sh qemu_session_evidence_missing_second_retire.log
sed 's/output=2 checksum=12847590821349875/output=2 checksum=8957873632062205093/' \
    "$FIXTURE_DIR/qemu_session_evidence_pass.log" >"$TEMP_DIR/qemu-identical-content.log"
"$ROOT_DIR/tools/verify_qemu_session_evidence.sh" \
    "$TEMP_DIR/qemu-identical-content.log" >/dev/null
# Two completion records naming one output is what a mirror group emits, and it
# must not satisfy a gate whose claim is two independent outputs.
sed 's/^sophia_live_output schema=1 status=complete output=2 /sophia_live_output schema=1 status=complete output=1 /' \
    "$FIXTURE_DIR/qemu_session_evidence_pass.log" >"$TEMP_DIR/qemu-mirrored-output.log"
if "$ROOT_DIR/tools/verify_qemu_session_evidence.sh" \
    "$TEMP_DIR/qemu-mirrored-output.log" >/dev/null 2>&1; then
    echo "QEMU verifier accepted two completion records for one output" >&2
    exit 1
fi
expect_fail tools/verify_qemu_session_evidence.sh qemu_session_evidence_vsync_overlap.log

# The per-head composition shape, taken from a real green QEMU run rather than
# written by hand. Its second output holds no windows and therefore exports no
# nonzero pixels, which is the case the older per-output pixel demand refused.
expect_pass tools/verify_qemu_session_evidence.sh qemu_session_evidence_per_head_pass.log
expect_fail_reason tools/verify_qemu_session_evidence.sh \
    qemu_session_evidence_per_head_no_pixels.log \
    "no nonzero exports on any output"
expect_fail_reason tools/verify_qemu_session_evidence.sh \
    qemu_session_evidence_per_head_pipeline_mismatch.log \
    "inconsistent direct-write/persistent-GL resource counters"
expect_fail_reason tools/verify_qemu_session_evidence.sh \
    qemu_session_evidence_per_head_target_recreation.log \
    "did not preserve bounded native upload resources"
expect_fail_reason tools/verify_qemu_session_evidence.sh \
    qemu_session_evidence_per_head_missing_present_routing.log \
    "unknown or missing field"
expect_pass tools/verify_qemu_emergency_recovery_evidence.sh qemu_emergency_recovery_pass.log
expect_fail tools/verify_qemu_emergency_recovery_evidence.sh qemu_emergency_recovery_missing_guard_trigger.log

# Both taken from real guest runs on 2026-09-23: the pass, and the blank-row
# drag the scenario's red half makes (SOPHIA_QEMU_XTEST_ROW=5), which xterm
# trims to nothing so the driver's text check fails and the session with it.
expect_pass tools/verify_qemu_xtest_selection_evidence.sh qemu_xtest_selection_evidence_pass.log
expect_fail_reason tools/verify_qemu_xtest_selection_evidence.sh \
    qemu_xtest_selection_evidence_blank_row.log \
    "missing clean guest completion"
# The counters are the verdict; a matched driver line must not pass without them.
sed 's/owner_changes=1 conversions=3/owner_changes=0 conversions=3/' \
    "$FIXTURE_DIR/qemu_xtest_selection_evidence_pass.log" > "$TEMP_DIR/xtest-no-owner.log"
if "$ROOT_DIR/tools/verify_qemu_xtest_selection_evidence.sh" "$TEMP_DIR/xtest-no-owner.log" >/dev/null 2>&1; then
    echo "xtest-selection verifier accepted a run in which nothing took PRIMARY" >&2
    exit 1
fi
sed 's/owner_changes=1 conversions=3/owner_changes=1 conversions=1/' \
    "$FIXTURE_DIR/qemu_xtest_selection_evidence_pass.log" > "$TEMP_DIR/xtest-no-paste.log"
if "$ROOT_DIR/tools/verify_qemu_xtest_selection_evidence.sh" "$TEMP_DIR/xtest-no-paste.log" >/dev/null 2>&1; then
    echo "xtest-selection verifier accepted a run in which nothing asked for PRIMARY" >&2
    exit 1
fi
sed 's/stdout_match=true/stdout_match=false/' \
    "$FIXTURE_DIR/qemu_xtest_selection_evidence_pass.log" > "$TEMP_DIR/xtest-stdout-mismatch.log"
if "$ROOT_DIR/tools/verify_qemu_xtest_selection_evidence.sh" "$TEMP_DIR/xtest-stdout-mismatch.log" >/dev/null 2>&1; then
    echo "xtest-selection verifier accepted a run whose driver verdict did not match" >&2
    exit 1
fi

expect_pass tools/verify_vrr_hardware_evidence.sh vrr_hardware_evidence_pass.log
expect_fail tools/verify_vrr_hardware_evidence.sh vrr_hardware_evidence_missing_fallback.log

echo "atomic scanout verifier fixtures passed"
