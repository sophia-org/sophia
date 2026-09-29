#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT HUP INT TERM

alloy_archive=${SOPHIA_ALLOY_ARCHIVE:-}
alloy_sha256=5a5494a4bac6e243e471590bb44a91e25a35794a5af1ae1f332be30b9c54a9e7
z3_bin=${SOPHIA_Z3:-z3}

if [ -z "$alloy_archive" ] || [ ! -f "$alloy_archive" ]; then
    echo "set SOPHIA_ALLOY_ARCHIVE to the Alloy 6.2.0 Linux amd64 archive" >&2
    exit 1
fi

actual_alloy_sha256=$(sha256sum "$alloy_archive" | awk '{print $1}')
if [ "$actual_alloy_sha256" != "$alloy_sha256" ]; then
    echo "Alloy archive SHA-256 mismatch" >&2
    echo "expected: $alloy_sha256" >&2
    echo "actual:   $actual_alloy_sha256" >&2
    exit 1
fi

tar -xzf "$alloy_archive" -C "$work"
alloy_bin=$work/alloy-6.2.0/bin/alloy
if [ "$($alloy_bin version)" != "6.2.0" ]; then
    echo "extracted Alloy command does not report version 6.2.0" >&2
    exit 1
fi

if ! z3_version=$($z3_bin --version 2>/dev/null); then
    echo "Z3 command is unavailable: $z3_bin" >&2
    exit 1
fi
case "$z3_version" in
    "Z3 version 4.16.0"*) ;;
    *)
        echo "stable architecture gate requires Z3 4.16.0; got: $z3_version" >&2
        exit 1
        ;;
esac

run_alloy() {
    model=$1
    command=$2
    expected=$3
    output=$work/alloy-$command
    log=$work/alloy-$command.log

    "$alloy_bin" exec -q -s sat4j -y 20 -c "$command" -t json \
        -o "$output" "$root/validation/architecture/alloy/$model" >"$log" 2>&1 || {
        echo "Alloy failed: $model / $command" >&2
        sed -n '1,160p' "$log" >&2
        exit 1
    }
    if [ ! -f "$output/receipt.json" ]; then
        echo "Alloy emitted no receipt: $model / $command" >&2
        exit 1
    fi
    solutions=$(find "$output" -name '*-solution-0.json' -type f | wc -l)
    case "$expected:$solutions" in
        unsat:0|sat:1) ;;
        *)
            echo "Alloy result mismatch: $model / $command expected $expected, solution files=$solutions" >&2
            sed -n '1,160p' "$log" >&2
            exit 1
            ;;
    esac
    echo "alloy $expected: $command"
}

run_smt() {
    model=$1
    actual=$work/$model.actual
    expected=$root/validation/architecture/smt/$model.expected
    (
        cd "$root"
        "$z3_bin" "validation/architecture/smt/$model.smt2"
    ) >"$actual"
    if rg -n '^(unknown|\(error )' "$actual" >/dev/null; then
        echo "Z3 returned unknown or an error: $model" >&2
        sed -n '1,200p' "$actual" >&2
        exit 1
    fi
    if ! diff -u "$expected" "$actual"; then
        echo "Z3 result mismatch: $model" >&2
        exit 1
    fi
    echo "z3 expected results: $model"
}

cd "$root"
cargo run --offline -q -p sophia-policy-protocol-gen -- --check

run_alloy AuthorityTopology.als NoAmbientOrInferredRoleAuthority unsat
run_alloy AuthorityTopology.als CrossNamespaceAccessRequiresPortalGrant unsat
run_alloy AuthorityTopology.als CoordinateAuthorityIsIndependentlyIssued unsat
run_alloy AuthorityTopology.als WmCannotObserveApplicationMetadata unsat
run_alloy AuthorityTopology.als WmProtectionDomainCannotComposeMetadataRoles unsat
run_alloy AuthorityTopology.als WmProtectionDomainCannotObserveApplicationMetadata unsat
run_alloy AuthorityTopology.als AmbientRoleAttack sat
run_alloy AuthorityTopology.als CrossNamespaceWithoutPortalAttack sat
run_alloy AuthorityTopology.als SelfIssuedCoordinateAttack sat
run_alloy AuthorityTopology.als WmMetadataAttack sat
run_alloy AuthorityTopology.als CombinedWmShellDomainAttack sat
run_alloy AuthorityTopology.als WmDomainMetadataCollusionAttack sat

run_alloy ActionCapabilityTopology.als AcceptedActionsMatchIssuerFamily unsat
run_alloy ActionCapabilityTopology.als AcceptedRecipientsMatchActionFamily unsat
run_alloy ActionCapabilityTopology.als StaleRevokedOrExpiredCapabilitiesAreRejected unsat
run_alloy ActionCapabilityTopology.als AcceptedTargetsMatchCapability unsat
run_alloy ActionCapabilityTopology.als AcceptedActivationIdentitiesDoNotReplay unsat
run_alloy ActionCapabilityTopology.als ValidScopedAction sat
run_alloy ActionCapabilityTopology.als CrossIssuerTypeConfusionAttack sat
run_alloy ActionCapabilityTopology.als WrongRecipientRoleAttack sat
run_alloy ActionCapabilityTopology.als StaleCapabilityAttack sat
run_alloy ActionCapabilityTopology.als RevokedCapabilityAttack sat
run_alloy ActionCapabilityTopology.als TargetGenerationSubstitutionAttack sat
run_alloy ActionCapabilityTopology.als RecipientSubstitutionAttack sat
run_alloy ActionCapabilityTopology.als ActivationReplayAttack sat

run_alloy PolicyOperationBinding.als AcceptedOperationUsesCommittedBindingSlot unsat
run_alloy PolicyOperationBinding.als AcceptedOperationUsesAdvertisedOpaqueToken unsat
run_alloy PolicyOperationBinding.als TargetDeliveryRequiresPermission unsat
run_alloy PolicyOperationBinding.als ValidOperation sat
run_alloy PolicyOperationBinding.als NumericActionInferenceAttack sat
run_alloy PolicyOperationBinding.als OpaqueTokenSubstitutionAttack sat
run_alloy PolicyOperationBinding.als UnauthorizedTargetAttack sat

run_alloy PresentedTargetTopology.als DeliveredTargetsAreOwnedVisibleAndModal unsat
run_alloy PresentedTargetTopology.als HigherTrustAndTieBreakCannotBeIntercepted unsat
run_alloy PresentedTargetTopology.als PresentedTargetIdentitiesAreUnique unsat
run_alloy PresentedTargetTopology.als CoordinateGrantsAreIndependentAndLocal unsat
run_alloy PresentedTargetTopology.als TargetOutsideAllocationAttack sat
run_alloy PresentedTargetTopology.als OccludedTargetAttack sat
run_alloy PresentedTargetTopology.als LowerTrustInterceptionAttack sat
run_alloy PresentedTargetTopology.als AmbiguousWithoutTieBreakAttack sat
run_alloy PresentedTargetTopology.als ReusedTargetIdentityAttack sat
run_alloy PresentedTargetTopology.als SelfIssuedGrantAttack sat

run_alloy OutputTopologyPublication.als AllConsumersShareCompleteEpoch unsat
run_alloy OutputTopologyPublication.als InputUsesPresentedCurrentTopology unsat
run_alloy OutputTopologyPublication.als OutputIdentityGenerationIsUnique unsat
run_alloy OutputTopologyPublication.als PublishedTopologyIsNonempty unsat
run_alloy OutputTopologyPublication.als ValidPublication sat
run_alloy OutputTopologyPublication.als MixedConsumerEpochAttack sat
run_alloy OutputTopologyPublication.als PartialOutputPublicationAttack sat
run_alloy OutputTopologyPublication.als StalePresentedInputAttack sat
run_alloy OutputTopologyPublication.als UncommittedPolicyInputAttack sat
run_alloy OutputTopologyPublication.als ReusedOutputGenerationAttack sat
run_alloy OutputTopologyPublication.als EmptyPublishedTopologyAttack sat

run_smt TargetGeometryAndDisclosure
run_smt WmFileRowsBounds
run_smt PolicyPresentationGeometry
run_smt OutputTopologyGeometry

if [ -n "${SOPHIA_Z3_DIFFERENTIAL:-}" ]; then
    differential_version=$("$SOPHIA_Z3_DIFFERENTIAL" --version 2>/dev/null || true)
    case "$differential_version" in
        "Z3 version 5."*) ;;
        *)
            echo "SOPHIA_Z3_DIFFERENTIAL must name a Z3 5.x command; got: $differential_version" >&2
            exit 1
            ;;
    esac
    stable_z3=$z3_bin
    z3_bin=$SOPHIA_Z3_DIFFERENTIAL
    run_smt TargetGeometryAndDisclosure
    run_smt WmFileRowsBounds
    run_smt PolicyPresentationGeometry
    run_smt OutputTopologyGeometry
    z3_bin=$stable_z3
    echo "optional Z3 5.x differential matched the stable gate"
fi
