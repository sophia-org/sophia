#!/usr/bin/env bash
set -euo pipefail

if (( $# < 2 || $# > 4 )); then
    echo "usage: verify_frame_fed_output_evidence.sh SUCCESS_LOG ROLLBACK_LOG [SUCCESS_TEXT] [ROLLBACK_TEXT]" >&2
    exit 2
fi

success_log="$1"
rollback_log="$2"
success_text="${3:-outputapply}"
rollback_text="${4:-outputrollback}"

for evidence in "$success_log" "$rollback_log"; do
    [[ -s "$evidence" ]] || {
        echo "frame-fed output evidence is missing: $evidence" >&2
        exit 1
    }
done
for proof_text in "$success_text" "$rollback_text"; do
    [[ "$proof_text" =~ ^[a-z]{1,24}$ ]] || {
        echo "frame-fed output proof text must contain 1-24 lowercase ASCII letters" >&2
        exit 2
    }
done

fail() {
    echo "frame-fed output evidence: $*" >&2
    exit 1
}

identity_pattern='^sophia_frame_fed_output_gate schema=1 status=phase_started phase=(success|rollback) source_commit=[0-9a-f]{40} hagia_commit=[0-9a-f]{40} sophia_sha256=[0-9a-f]{64} hagia_sha256=[0-9a-f]{64} core_sha256=[0-9a-f]{64} profile_sha256=[0-9a-f]{64} connectors_sha256=[0-9a-f]{64}$'
for phase_and_log in "success:$success_log" "rollback:$rollback_log"; do
    phase="${phase_and_log%%:*}"
    evidence="${phase_and_log#*:}"
    [[ "$(grep -Ec "$identity_pattern" "$evidence" || true)" == 1 ]] \
        || fail "$phase log must contain one exact source, binary, configuration, and connector identity"
    grep -Eq "^sophia_frame_fed_output_gate schema=1 status=phase_started phase=$phase " "$evidence" \
        || fail "$phase log carries the wrong phase identity"
    grep -Fxq "sophia_frame_fed_output_gate schema=1 status=phase_passed phase=$phase exit=0" "$evidence" \
        || fail "$phase log lacks its passing gate record"
    grep -Eq '^sophia_live_session schema=(16|18) status=bounded_complete .* native_in_flight=false native_cleanup_pending=false .* wm_degraded=false ' "$evidence" \
        || fail "$phase log lacks bounded clean session completion"
    grep -Fxq 'sophia_live_session_health schema=1 status=clean protocol_errors=0 pending_wm=0 pending_actions=0 pending_input=0 wm_degraded=false' "$evidence" \
        || fail "$phase log lacks clean session health"
    grep -Fxq 'sophia_live_output_topology_health schema=1 status=clean quarantined=false' "$evidence" \
        || fail "$phase log lacks clean output-topology health"
    grep -Fxq 'sophia_live_session_cleanup schema=1 status=clean app_groups=0 frontend_workers=0 namespace=revoked xauthority=removed' "$evidence" \
        || fail "$phase log lacks clean process cleanup"
    if grep -Eq '(^Error:|panicked at|status=(failed|degraded|rollback_failed)([[:space:]]|$))' "$evidence"; then
        fail "$phase log contains a runtime failure or degradation"
    fi
done

identity_payload() {
    local evidence="$1"
    grep -E "$identity_pattern" "$evidence" | sed -E 's/ phase=(success|rollback) / phase=proof /'
}
[[ "$(identity_payload "$success_log")" == "$(identity_payload "$rollback_log")" ]] \
    || fail "success and rollback logs do not bind the same build and configuration"

transaction_from_startup() {
    local evidence="$1" transactions
    mapfile -t transactions < <(
        sed -n 's/.*sophia_live_output_authority schema=3 status=startup_effect_pending transaction=\([0-9][0-9]*\) .*/\1/p' "$evidence"
    )
    (( ${#transactions[@]} == 1 )) \
        || fail "$(basename "$evidence") must contain one startup output transaction"
    printf '%s\n' "${transactions[0]}"
}

line_for() {
    local evidence="$1" pattern="$2" description="$3" lines
    mapfile -t lines < <(grep -nE "$pattern" "$evidence" | cut -d: -f1 || true)
    (( ${#lines[@]} == 1 )) || fail "$description must occur exactly once"
    printf '%s\n' "${lines[0]}"
}

require_order() {
    local previous=0 current
    for current in "$@"; do
        (( current > previous )) || fail "required output-authority events are out of order"
        previous="$current"
    done
}

success_transaction="$(transaction_from_startup "$success_log")"
success_start="$(line_for "$success_log" "status=startup_effect_pending transaction=$success_transaction " 'success startup candidate')"
success_prepare="$(line_for "$success_log" "status=resource_preparation_started transaction=$success_transaction " 'success resource preparation')"
success_apply="$(line_for "$success_log" "status=apply_started transaction=$success_transaction " 'success KMS apply')"
success_install="$(line_for "$success_log" "status=candidate_installed transaction=$success_transaction " 'success candidate installation')"
success_present="$(line_for "$success_log" "status=first_presented transaction=$success_transaction " 'success first presentation')"
success_frontend="$(line_for "$success_log" "status=frontend_candidate_published transaction=$success_transaction " 'success frontend publication')"
success_settle="$(line_for "$success_log" "status=settled_locally transaction=$success_transaction outcome=Committed .* preserved_topology=false$" 'success local settlement')"
mapfile -t success_topology_epochs < <(
    sed -n "s/.*status=settled_locally transaction=$success_transaction outcome=Committed topology_epoch=\([0-9][0-9]*\) .*/\1/p" "$success_log"
)
(( ${#success_topology_epochs[@]} == 1 && success_topology_epochs[0] > 0 )) \
    || fail "success local settlement must carry one nonzero topology epoch"
success_topology_epoch="${success_topology_epochs[0]}"
# Snapshot publication is a distinct unsolicited transport update, so its
# transaction belongs to that transport rather than to the private startup
# authority transaction. The shared fact is the committed topology epoch.
success_snapshot="$(line_for "$success_log" "status=committed_snapshot_published transaction=[1-9][0-9]* topology_epoch=$success_topology_epoch transport_published=true$" 'success committed snapshot publication')"
success_commit="$(line_for "$success_log" "status=committed transaction=$success_transaction " 'success topology commit')"
success_input="$(line_for "$success_log" "^sophia_live_session_input schema=2 status=complete source=physical text=$success_text " 'success physical confirmation')"
require_order "$success_start" "$success_prepare" "$success_apply" "$success_install" \
    "$success_present" "$success_frontend" "$success_settle" "$success_snapshot" \
    "$success_commit" "$success_input"
if grep -Eq "status=(proof_rollback_triggered|rollback_started|rolled_back) transaction=$success_transaction " "$success_log"; then
    fail "success phase contains rollback evidence"
fi

rollback_transaction="$(transaction_from_startup "$rollback_log")"
rollback_start="$(line_for "$rollback_log" "status=startup_effect_pending transaction=$rollback_transaction " 'rollback startup candidate')"
rollback_prepare="$(line_for "$rollback_log" "status=resource_preparation_started transaction=$rollback_transaction " 'rollback resource preparation')"
rollback_apply="$(line_for "$rollback_log" "status=apply_started transaction=$rollback_transaction " 'rollback KMS apply')"
rollback_trigger="$(line_for "$rollback_log" "status=proof_rollback_triggered transaction=$rollback_transaction boundary=after_apply .* candidate_installed=false published=false$" 'post-apply rollback trigger')"
rollback_started="$(line_for "$rollback_log" "status=rollback_started transaction=$rollback_transaction reason=proof_after_apply published=false$" 'proof rollback start')"
rollback_settle="$(line_for "$rollback_log" "status=settled_locally transaction=$rollback_transaction outcome=RolledBack .* preserved_topology=true$" 'rollback local settlement')"
rollback_complete="$(line_for "$rollback_log" "status=rolled_back transaction=$rollback_transaction .* published=false input=enabled$" 'rollback completion')"
rollback_input="$(line_for "$rollback_log" "^sophia_live_session_input schema=2 status=complete source=physical text=$rollback_text " 'rollback physical confirmation')"
require_order "$rollback_start" "$rollback_prepare" "$rollback_apply" "$rollback_trigger" \
    "$rollback_started" "$rollback_settle" "$rollback_complete" "$rollback_input"

if grep -Eq 'status=committed_snapshot_published ' "$rollback_log"; then
    fail "rollback phase crossed forbidden boundary: committed_snapshot_published"
fi
for forbidden in candidate_installed first_presented frontend_candidate_published committed; do
    if grep -Eq "status=$forbidden transaction=$rollback_transaction " "$rollback_log"; then
        fail "rollback phase crossed forbidden boundary: $forbidden"
    fi
done

echo "sophia_frame_fed_output_evidence schema=1 status=verified success_transaction=$success_transaction rollback_transaction=$rollback_transaction boundary=after_apply"
