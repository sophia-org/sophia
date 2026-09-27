#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERIFIER="$ROOT_DIR/tools/verify_frame_fed_output_evidence.sh"
ARCHIVER="$ROOT_DIR/tools/archive_frame_fed_output_physical_run.sh"
ARCHIVE_VERIFIER="$ROOT_DIR/tools/verify_frame_fed_output_physical_archive.sh"
work="$(mktemp -d)"
trap 'rm -rf -- "$work"' EXIT

source_commit="$(git -C "$ROOT_DIR" rev-parse HEAD)"
# The binary is synthetic; exercise real signature checks using this
# repository's commit instead of requiring an unrelated product checkout.
hagia_root="$work/policy-source"
# The archiver requires a checkout with a .git directory. A local clone also
# works when this test itself runs from a linked worktree (.git is a file).
git -c init.templateDir= clone --quiet --shared --no-checkout "$ROOT_DIR" "$hagia_root"
hagia_commit="$source_commit"
sophia_bin="$work/sophia"
hagia_bin="$work/hagia"
cp /usr/bin/true "$sophia_bin"
cp /usr/bin/false "$hagia_bin"
# These fixtures exist in the signed parent commit, allowing the archive test
# to exercise commit-blob verification while config-path changes are still
# uncommitted in the working tree.
core_config="$ROOT_DIR/tools/fixtures/direct_scanout_core.kdl"
desktop_profile="$ROOT_DIR/tools/fixtures/mixed_output_probe.kdl"
connectors="$work/connectors.txt"
printf '%s\n' \
    'DP-1 status=connected preferred=2560x1440' \
    'DP-2 status=connected preferred=1920x1080' >"$connectors"
sophia_sha256="$(sha256sum "$sophia_bin" | awk '{ print $1 }')"
hagia_sha256="$(sha256sum "$hagia_bin" | awk '{ print $1 }')"
core_sha256="$(sha256sum "$core_config" | awk '{ print $1 }')"
profile_sha256="$(sha256sum "$desktop_profile" | awk '{ print $1 }')"
connectors_sha256="$(sha256sum "$connectors" | awk '{ print $1 }')"
identity="source_commit=$source_commit hagia_commit=$hagia_commit sophia_sha256=$sophia_sha256 hagia_sha256=$hagia_sha256 core_sha256=$core_sha256 profile_sha256=$profile_sha256 connectors_sha256=$connectors_sha256"

write_success() {
    local target="$1"
    {
        echo 'sophia_live_output_authority schema=3 status=startup_effect_pending transaction=18446744073709551615 preserved_topology=true'
        echo 'sophia_live_output_authority schema=2 status=resource_preparation_started transaction=18446744073709551615 base_epoch=1 candidate_epoch=2 heads=2 outputs=2 phase=Preparing candidate_prepared=false rollback_prepared=false kms_submits=0 published=false input=quarantined'
        echo 'sophia_live_output_authority schema=2 status=apply_started transaction=18446744073709551615 heads=2 cards=ordered published=false'
        echo 'sophia_live_output_authority schema=2 status=candidate_installed transaction=18446744073709551615 card=0 outputs=2 first_frames=2 published=false rollback_retained=true'
        echo 'sophia_live_output_authority schema=2 status=first_presented transaction=18446744073709551615 outputs=2 published=false rollback_retained=true'
        echo 'sophia_live_output_authority schema=3 status=frontend_candidate_published transaction=18446744073709551615 generation=2 published=false rollback_retained=true'
        echo 'sophia_live_output_authority schema=3 status=settled_locally transaction=18446744073709551615 outcome=Committed topology_epoch=2 reason="desktop profile startup" preserved_topology=false'
        echo 'sophia_live_output_authority schema=2 status=committed_snapshot_published transaction=2 topology_epoch=2 transport_published=true'
        echo 'sophia_live_output_authority schema=2 status=committed transaction=18446744073709551615 topology_epoch=2 outputs=2 policy_required=true input=quarantined'
        echo 'sophia_live_session_input schema=2 status=complete source=physical text=outputapply expected_events=12 matched_events=12 pixel_change=true'
        echo 'sophia_live_session schema=16 status=bounded_complete display=:296 elapsed_msec=1000 native_in_flight=false native_cleanup_pending=false physical_input=enabled wm_policy=enabled wm_restarts=0 wm_degraded=false output_update=applied'
        echo 'sophia_live_session_health schema=1 status=clean protocol_errors=0 pending_wm=0 pending_actions=0 pending_input=0 wm_degraded=false'
        echo 'sophia_live_output_topology_health schema=1 status=clean quarantined=false'
        echo 'sophia_live_session_cleanup schema=1 status=clean app_groups=0 frontend_workers=0 namespace=revoked xauthority=removed'
        printf 'sophia_frame_fed_output_gate schema=1 status=phase_started phase=success %s\n' "$identity"
        echo 'sophia_frame_fed_output_gate schema=1 status=phase_passed phase=success exit=0'
    } >"$target"
}

write_rollback() {
    local target="$1"
    {
        echo 'sophia_live_output_authority schema=3 status=startup_effect_pending transaction=18446744073709551615 preserved_topology=true'
        echo 'sophia_live_output_authority schema=2 status=resource_preparation_started transaction=18446744073709551615 base_epoch=1 candidate_epoch=2 heads=2 outputs=2 phase=Preparing candidate_prepared=false rollback_prepared=false kms_submits=0 published=false input=quarantined'
        echo 'sophia_live_output_authority schema=2 status=apply_started transaction=18446744073709551615 heads=2 cards=ordered published=false'
        echo 'sophia_live_output_authority schema=3 status=proof_rollback_triggered transaction=18446744073709551615 boundary=after_apply card=0 heads=2 candidate_installed=false published=false'
        echo 'sophia_live_output_authority schema=3 status=rollback_started transaction=18446744073709551615 reason=proof_after_apply published=false'
        echo 'sophia_live_output_authority schema=3 status=settled_locally transaction=18446744073709551615 outcome=RolledBack topology_epoch=1 reason="desktop profile startup" preserved_topology=true'
        echo 'sophia_live_output_authority schema=2 status=rolled_back transaction=18446744073709551615 card=0 reason="proof requested rollback after KMS apply" published=false input=enabled'
        echo 'sophia_live_session_input schema=2 status=complete source=physical text=outputrollback expected_events=15 matched_events=15 pixel_change=true'
        echo 'sophia_live_session schema=16 status=bounded_complete display=:297 elapsed_msec=1000 native_in_flight=false native_cleanup_pending=false physical_input=enabled wm_policy=enabled wm_restarts=0 wm_degraded=false output_update=applied'
        echo 'sophia_live_session_health schema=1 status=clean protocol_errors=0 pending_wm=0 pending_actions=0 pending_input=0 wm_degraded=false'
        echo 'sophia_live_output_topology_health schema=1 status=clean quarantined=false'
        echo 'sophia_live_session_cleanup schema=1 status=clean app_groups=0 frontend_workers=0 namespace=revoked xauthority=removed'
        printf 'sophia_frame_fed_output_gate schema=1 status=phase_started phase=rollback %s\n' "$identity"
        echo 'sophia_frame_fed_output_gate schema=1 status=phase_passed phase=rollback exit=0'
    } >"$target"
}

success="$work/success.log"
rollback="$work/rollback.log"
write_success "$success"
write_rollback "$rollback"
"$VERIFIER" "$success" "$rollback" outputapply outputrollback >/dev/null

expect_evidence_failure() {
    if "$VERIFIER" "$1" "$2" outputapply outputrollback >/dev/null 2>&1; then
        echo "frame-fed output verifier accepted mutation: $3" >&2
        exit 1
    fi
}

success_requirements=(
    startup_effect_pending resource_preparation_started apply_started candidate_installed
    first_presented frontend_candidate_published settled_locally committed_snapshot_published
    'status=committed transaction=' 'text=outputapply' 'status=bounded_complete'
    'sophia_live_session_health' 'sophia_live_output_topology_health' 'sophia_live_session_cleanup'
    'phase_started phase=success' 'phase_passed phase=success'
)
for requirement in "${success_requirements[@]}"; do
    write_success "$work/mutated-success.log"
    sed -i "\|$requirement|d" "$work/mutated-success.log"
    expect_evidence_failure "$work/mutated-success.log" "$rollback" "missing success $requirement"
done

rollback_requirements=(
    startup_effect_pending resource_preparation_started apply_started proof_rollback_triggered
    rollback_started settled_locally rolled_back 'text=outputrollback' 'status=bounded_complete'
    'sophia_live_session_health' 'sophia_live_output_topology_health' 'sophia_live_session_cleanup'
    'phase_started phase=rollback' 'phase_passed phase=rollback'
)
for requirement in "${rollback_requirements[@]}"; do
    write_rollback "$work/mutated-rollback.log"
    sed -i "\|$requirement|d" "$work/mutated-rollback.log"
    expect_evidence_failure "$success" "$work/mutated-rollback.log" "missing rollback $requirement"
done

write_rollback "$work/mutated-rollback.log"
sed -i '/proof_rollback_triggered/p' "$work/mutated-rollback.log"
expect_evidence_failure "$success" "$work/mutated-rollback.log" 'duplicate rollback trigger'
write_rollback "$work/mutated-rollback.log"
sed -i '/rollback_started/i sophia_live_output_authority schema=2 status=candidate_installed transaction=18446744073709551615 card=0 outputs=2 first_frames=2 published=false rollback_retained=true' "$work/mutated-rollback.log"
expect_evidence_failure "$success" "$work/mutated-rollback.log" 'candidate installation during rollback'
write_rollback "$work/mutated-rollback.log"
sed -i 's/hagia_sha256=[0-9a-f]*/hagia_sha256=cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc/' "$work/mutated-rollback.log"
expect_evidence_failure "$success" "$work/mutated-rollback.log" 'phase identity mismatch'
write_success "$work/mutated-success.log"
sed -i 's/status=committed_snapshot_published transaction=2 topology_epoch=2/status=committed_snapshot_published transaction=2 topology_epoch=3/' "$work/mutated-success.log"
expect_evidence_failure "$work/mutated-success.log" "$rollback" 'committed snapshot epoch mismatch'
write_rollback "$work/mutated-rollback.log"
sed -i '/status=bounded_complete/i sophia_live_output_authority schema=2 status=committed_snapshot_published transaction=2 topology_epoch=1 transport_published=true' "$work/mutated-rollback.log"
expect_evidence_failure "$success" "$work/mutated-rollback.log" 'snapshot publication during rollback'

archive_output="$(
    XDG_STATE_HOME="$work/state" \
    SOPHIA_FRAME_FED_OUTPUT_SOPHIA_BIN="$sophia_bin" \
    SOPHIA_FRAME_FED_OUTPUT_HAGIA_BIN="$hagia_bin" \
    SOPHIA_FRAME_FED_OUTPUT_CORE_CONFIG="$core_config" \
    SOPHIA_FRAME_FED_OUTPUT_DESKTOP_PROFILE="$desktop_profile" \
    SOPHIA_HAGIA_ROOT="$hagia_root" \
        "$ARCHIVER" "$success" "$rollback" "$connectors" outputapply outputrollback
)"
run_dir="${archive_output##*: }"
SOPHIA_HAGIA_ROOT="$hagia_root" "$ARCHIVE_VERIFIER" "$run_dir" >/dev/null

if XDG_STATE_HOME="$work/state" \
    SOPHIA_FRAME_FED_OUTPUT_SOPHIA_BIN="$sophia_bin" \
    SOPHIA_FRAME_FED_OUTPUT_HAGIA_BIN="$hagia_bin" \
    SOPHIA_FRAME_FED_OUTPUT_CORE_CONFIG="$core_config" \
    SOPHIA_FRAME_FED_OUTPUT_DESKTOP_PROFILE="$desktop_profile" \
    SOPHIA_HAGIA_ROOT="$hagia_root" \
        "$ARCHIVER" "$success" "$rollback" "$connectors" outputapply outputrollback \
        >/dev/null 2>&1; then
    echo 'frame-fed output archiver accepted duplicate evidence' >&2
    exit 1
fi

cp "$run_dir/manifest" "$work/manifest"
sed -i 's/^sophia_binary_sha256=.*/sophia_binary_sha256=dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd/' "$run_dir/manifest"
(
    cd "$run_dir"
    sha256sum connectors.txt core.kdl desktop-profile.kdl manifest result.kdl rollback.log success.log >SHA256SUMS
)
if SOPHIA_HAGIA_ROOT="$hagia_root" "$ARCHIVE_VERIFIER" "$run_dir" >/dev/null 2>&1; then
    echo 'frame-fed output archive accepted a binary digest different from its evidence' >&2
    exit 1
fi

cp "$work/manifest" "$run_dir/manifest"
sed -i 's|^desktop_profile_path=.*|desktop_profile_path=validation/desktop-comparison/profiles/hagia.kdl|' "$run_dir/manifest"
(
    cd "$run_dir"
    sha256sum connectors.txt core.kdl desktop-profile.kdl manifest result.kdl rollback.log success.log >SHA256SUMS
)
if SOPHIA_HAGIA_ROOT="$hagia_root" "$ARCHIVE_VERIFIER" "$run_dir" >/dev/null 2>&1; then
    echo 'frame-fed output archive accepted a profile outside its signed source blob' >&2
    exit 1
fi

cp "$work/manifest" "$run_dir/manifest"
printf '\n' >>"$run_dir/connectors.txt"
if SOPHIA_HAGIA_ROOT="$hagia_root" "$ARCHIVE_VERIFIER" "$run_dir" >/dev/null 2>&1; then
    echo 'frame-fed output archive accepted checksum corruption' >&2
    exit 1
fi

echo 'Frame-fed output evidence and archive verifier fixtures passed.'
