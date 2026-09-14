#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MODEL_DIR="$ROOT_DIR/validation/tla"
EXPECTED_SHA256="936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88"
JAR_PATH="${SOPHIA_TLA2TOOLS_JAR:-}"

if [[ -z "$JAR_PATH" ]]; then
    echo "SOPHIA_TLA2TOOLS_JAR must name the pinned TLA+ Tools v1.7.4 jar" >&2
    exit 2
fi
if [[ "$JAR_PATH" != /* ]]; then
    echo "SOPHIA_TLA2TOOLS_JAR must be an absolute path" >&2
    exit 2
fi
if [[ ! -f "$JAR_PATH" ]]; then
    echo "TLA+ tools jar not found: $JAR_PATH" >&2
    exit 2
fi
if ! command -v java >/dev/null 2>&1; then
    echo "Java 11 or newer is required to run TLC" >&2
    exit 2
fi

actual_sha256="$(sha256sum "$JAR_PATH" | awk '{print $1}')"
if [[ "$actual_sha256" != "$EXPECTED_SHA256" ]]; then
    echo "TLA+ tools jar checksum mismatch" >&2
    echo "expected: $EXPECTED_SHA256" >&2
    echo "actual:   $actual_sha256" >&2
    exit 2
fi

java_major="$(java -version 2>&1 | sed -n '1s/.*version "\([0-9][0-9]*\).*/\1/p')"
if [[ -z "$java_major" ]] || (( java_major < 11 )); then
    echo "Java 11 or newer is required to run TLC" >&2
    exit 2
fi

TEMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TEMP_DIR"' EXIT
for model in RetainedCompositionAdmission NativeSessionLifecycle TabDescriptorPresentation VisualRetirement VisualRetirementSlots VisualDamageHistory StableBackingLease AdmissionRecovery PresentFrameOwnership PresentCopyOwnership PresentFlipOwnership PresentMixedOwnership SurfaceContentStream GeometryFeedback PolicyConnection PolicyProjection PolicyLifecycle PolicySettlementRecovery PolicyOutputSettlement PolicyRefreshLifecycle OutputTopologyLifecycle ShellObservation ShellDescriptorLifecycle ShellWorkAreaCoordination IndicatorTransfer IndicatorAction TargetResolvedInput TargetInputPacing InputAuthorityArbitration PointerGrabAdmission FrameServiceArbitration PageFlipCompletionPump PageFlipPresentationTracker SharedWorkerService CursorPlaneTransactionOwner MirrorHeadPacing PixelSilentAdmission ContinuousContentPresentation ShellContentLifecycle ShellContentBundleComposition ShellPresentedContentAction ShellGpuLaunchAdmission InputDeliveryRecovery XAuthorityShutdown; do
    cp "$MODEL_DIR/$model.tla" "$TEMP_DIR/"
    cp "$MODEL_DIR/$model.cfg" "$TEMP_DIR/"
    (
        cd "$TEMP_DIR"
        java -XX:+UseParallelGC -jar "$JAR_PATH" \
            -deadlock \
            -workers 1 \
            -fp 0 \
            -config "$model.cfg" \
            "$model.tla"
    )
done

for control in ShellGpuLaunchAdmissionKeepsLostGrant ShellGpuLaunchAdmissionReusesEpoch; do
    control_dir="$TEMP_DIR/$control"
    mkdir "$control_dir"
    cp "$MODEL_DIR/ShellGpuLaunchAdmission.tla" "$MODEL_DIR/$control.cfg" "$control_dir/"
    log="$control_dir/control.log"
    if (cd "$control_dir" && timeout 60 java -XX:+UseParallelGC -jar "$JAR_PATH" \
        -deadlock -workers 1 -fp 0 -config "$control.cfg" ShellGpuLaunchAdmission.tla) >"$log" 2>&1; then
        echo "TLA+ shell GPU admission negative control unexpectedly passed: $control" >&2
        exit 1
    fi
    case "$control" in
        ShellGpuLaunchAdmissionKeepsLostGrant) invariant=ActiveGrantNamesCurrentAuthority ;;
        ShellGpuLaunchAdmissionReusesEpoch) invariant=FreshGrantEpoch ;;
    esac
    grep -Fq "Invariant $invariant is violated." "$log" || { cat "$log"; exit 1; }
done

control=PageFlipPresentationTrackerMixedClock
control_dir="$TEMP_DIR/$control"
mkdir "$control_dir"
cp "$MODEL_DIR/PageFlipPresentationTracker.tla" "$control_dir/"
cp "$MODEL_DIR/$control.cfg" "$control_dir/"
log="$control_dir/control.log"
if (
    cd "$control_dir"
    java -XX:+UseParallelGC -jar "$JAR_PATH" \
        -deadlock \
        -workers 1 \
        -fp 0 \
        -config "$control.cfg" \
        PageFlipPresentationTracker.tla
) >"$log" 2>&1; then
    echo "TLA+ negative control unexpectedly passed: $control" >&2
    exit 1
fi
grep -Fq 'Invariant PhysicalTrackerOwnerAgreement is violated.' "$log" || {
    echo "TLA+ mixed-clock control failed for the wrong reason" >&2
    exit 1
}

for control in \
    XAuthorityShutdownPrematureExit \
    XAuthorityShutdownUnboundedIngress \
    XAuthorityShutdownRemovalWithoutSettlement; do
    control_dir="$TEMP_DIR/$control"
    mkdir "$control_dir"
    cp "$MODEL_DIR/XAuthorityShutdown.tla" "$control_dir/"
    cp "$MODEL_DIR/$control.cfg" "$control_dir/"
    log="$control_dir/control.log"
    if (
        cd "$control_dir"
        java -XX:+UseParallelGC -jar "$JAR_PATH" \
            -deadlock \
            -workers 1 \
            -fp 0 \
            -config "$control.cfg" \
            XAuthorityShutdown.tla
    ) >"$log" 2>&1; then
        echo "TLA+ negative control unexpectedly passed: $control" >&2
        exit 1
    fi
    case "$control" in
        XAuthorityShutdownPrematureExit)
            grep -Fq 'Invariant NoUncancellableEgress is violated.' "$log" || {
                echo "TLA+ premature-exit control failed for the wrong reason" >&2
                exit 1
            }
            ;;
        XAuthorityShutdownUnboundedIngress)
            grep -Fq 'Invariant BoundedProducerOwnership is violated.' "$log" || {
                echo "TLA+ unbounded-ingress control failed for the wrong reason" >&2
                exit 1
            }
            ;;
        XAuthorityShutdownRemovalWithoutSettlement)
            grep -Fq 'Invariant PendingHasLiveOwner is violated.' "$log" || {
                echo "TLA+ removal-without-settlement control failed for the wrong reason" >&2
                exit 1
            }
            ;;
    esac
done

# These configurations deliberately weaken exactly one progress or identity
# rule. A passing TLC run would mean the model no longer detects the failure it
# exists to exclude, so success is an error here.
for control in \
    ContinuousContentPresentationNoDrainFairness \
    ContinuousContentPresentationNoCompositionFairness \
    ContinuousContentPresentationUnaccountedSupersession \
    ContinuousContentPresentationNativeOwnerSupersession \
    ContinuousContentPresentationStaleRetirement; do
    control_dir="$TEMP_DIR/$control"
    mkdir "$control_dir"
    cp "$MODEL_DIR/ContinuousContentPresentation.tla" "$control_dir/"
    cp "$MODEL_DIR/$control.cfg" "$control_dir/"
    log="$control_dir/control.log"
    if (
        cd "$control_dir"
        java -XX:+UseParallelGC -jar "$JAR_PATH" \
            -deadlock \
            -workers 1 \
            -fp 0 \
            -config "$control.cfg" \
            ContinuousContentPresentation.tla
    ) >"$log" 2>&1; then
        echo "TLA+ negative control unexpectedly passed: $control" >&2
        exit 1
    fi
    case "$control" in
        ContinuousContentPresentationNoDrainFairness|ContinuousContentPresentationNoCompositionFairness)
            grep -Fq 'Error: Temporal properties were violated.' "$log" || {
                echo "TLA+ fairness control failed for the wrong reason: $control" >&2
                exit 1
            }
            ;;
        ContinuousContentPresentationUnaccountedSupersession)
            grep -Fq 'Invariant AllAcceptedUpdatesAccounted is violated.' "$log" || {
                echo "TLA+ supersession control failed for the wrong reason" >&2
                exit 1
            }
            ;;
        ContinuousContentPresentationNativeOwnerSupersession)
            grep -Fq 'Invariant NativeOwnersAreNotSuperseded is violated.' "$log" || {
                echo "TLA+ native-owner supersession control failed for the wrong reason" >&2
                exit 1
            }
            ;;
        ContinuousContentPresentationStaleRetirement)
            grep -Fq 'Invariant PresentedUpdatesRetired is violated.' "$log" || {
                echo "TLA+ stale-retirement control failed for the wrong reason" >&2
                exit 1
            }
            ;;
    esac
done

# Judging a retirement by the frame the scheduler names right now strands a
# frame this session submitted and then superseded, which is the verdict that
# ends a live session. A passing run here would mean the model no longer
# detects it.
control=PresentMixedOwnershipSchedulerOnly
control_dir="$TEMP_DIR/$control"
mkdir "$control_dir"
cp "$MODEL_DIR/PresentMixedOwnership.tla" "$control_dir/"
cp "$MODEL_DIR/$control.cfg" "$control_dir/"
log="$control_dir/control.log"
if (
    cd "$control_dir"
    java -XX:+UseParallelGC -jar "$JAR_PATH" \
        -deadlock \
        -workers 1 \
        -fp 0 \
        -config "$control.cfg" \
        PresentMixedOwnership.tla
) >"$log" 2>&1; then
    echo "TLA+ negative control unexpectedly passed: $control" >&2
    exit 1
fi
grep -Fq 'Invariant NoSubmittedFrameIsStranded is violated.' "$log" || {
    echo "TLA+ mixed-ownership control failed for the wrong reason" >&2
    exit 1
}

# Settling a superseded frame while leaving its cohort pending is the zombie
# that ended a session on the next recomposition. A passing run here would
# mean the model no longer detects it.
control=PresentMixedOwnershipZombieCohort
control_dir="$TEMP_DIR/$control"
mkdir "$control_dir"
cp "$MODEL_DIR/PresentMixedOwnership.tla" "$control_dir/"
cp "$MODEL_DIR/$control.cfg" "$control_dir/"
log="$control_dir/control.log"
if (
    cd "$control_dir"
    java -XX:+UseParallelGC -jar "$JAR_PATH" \
        -deadlock \
        -workers 1 \
        -fp 0 \
        -config "$control.cfg" \
        PresentMixedOwnership.tla
) >"$log" 2>&1; then
    echo "TLA+ negative control unexpectedly passed: $control" >&2
    exit 1
fi
# TLC 1.7.4 reports the generic form and never names the property, so this
# matches what the sibling fairness controls already grep for. The cfg declares
# PendingPresentSettles as its only PROPERTY, which is what keeps the generic
# message unambiguous here.
grep -Fq 'Error: Temporal properties were violated.' "$log" || {
    echo "TLA+ zombie-cohort control failed for the wrong reason" >&2
    exit 1
}

for control in TabDescriptorStaleCandidate TabDescriptorLostCapture; do
    control_dir="$TEMP_DIR/$control"
    mkdir "$control_dir"
    cp "$MODEL_DIR/TabDescriptorPresentation.tla" "$MODEL_DIR/$control.cfg" "$control_dir/"
    log="$control_dir/control.log"
    if (cd "$control_dir" && timeout 60 java -XX:+UseParallelGC -jar "$JAR_PATH" -deadlock -workers 1 -config "$control.cfg" TabDescriptorPresentation.tla) >"$log" 2>&1; then
        echo "TLA+ tab negative control unexpectedly passed: $control" >&2
        exit 1
    fi
    case "$control" in
        TabDescriptorStaleCandidate) invariant=CoherentPresentation ;;
        TabDescriptorLostCapture) invariant=ExactActivation ;;
    esac
    grep -Fq "Invariant $invariant is violated." "$log" || { cat "$log"; exit 1; }
done

for control in DiscardCounters ForgetFailure RequireResume; do
    control_dir="$TEMP_DIR/NativeSessionLifecycle$control"
    mkdir "$control_dir"
    cp "$MODEL_DIR/NativeSessionLifecycle.tla" "$control_dir/"
    cp "$MODEL_DIR/NativeSessionLifecycle$control.cfg" "$control_dir/"
    log="$control_dir/control.log"
    if (
        cd "$control_dir"
        timeout 30m java -XX:+UseParallelGC -jar "$JAR_PATH" -deadlock -workers 1 -fp 0 \
            -config "NativeSessionLifecycle$control.cfg" NativeSessionLifecycle.tla
    ) >"$log" 2>&1; then
        echo "TLA+ native lifecycle negative control unexpectedly passed: $control" >&2
        exit 1
    fi
    case "$control" in
        DiscardCounters) expected='Invariant EvidenceRetained is violated.' ;;
        ForgetFailure) expected='Invariant FailureRetained is violated.' ;;
        RequireResume) expected='Temporal properties were violated.' ;;
    esac
    grep -Fq "$expected" "$log" || {
        echo "TLA+ native lifecycle control failed for the wrong reason: $control" >&2
        exit 1
    }
done

control_dir="$TEMP_DIR/RetainedCompositionAdmissionSuppressed"
mkdir "$control_dir"
cp "$MODEL_DIR/RetainedCompositionAdmission.tla" "$MODEL_DIR/RetainedCompositionAdmissionSuppressed.cfg" "$control_dir/"
if (
    cd "$control_dir"
    timeout 30m java -XX:+UseParallelGC -jar "$JAR_PATH" -deadlock -workers 1 \
        -config RetainedCompositionAdmissionSuppressed.cfg RetainedCompositionAdmission.tla
) >"$control_dir/control.log" 2>&1; then
    echo "TLA+ retained-composition negative control unexpectedly passed" >&2
    exit 1
fi
grep -Fq 'Invariant CpuHasOnlyCpuSources is violated.' "$control_dir/control.log" || {
    echo "TLA+ retained-composition control failed for the wrong reason" >&2
    exit 1
}

# Admission must consume published prerequisites and retire cancelled ownership.
for control in PointerGrabAdmissionWithoutPrerequisite PointerGrabAdmissionReviveCancelled; do
    control_dir="$TEMP_DIR/$control"
    mkdir "$control_dir"
    cp "$MODEL_DIR/PointerGrabAdmission.tla" "$MODEL_DIR/$control.cfg" "$control_dir/"
    log="$control_dir/control.log"
    if (
        cd "$control_dir"
        timeout 30m java -XX:+UseParallelGC -jar "$JAR_PATH" \
            -deadlock -workers 1 -fp 0 -config "$control.cfg" PointerGrabAdmission.tla
    ) >"$log" 2>&1; then
        echo "TLA+ grab-admission negative control unexpectedly passed: $control" >&2
        exit 1
    fi
    case "$control" in
        PointerGrabAdmissionWithoutPrerequisite) invariant=AppliedBeforeGrant ;;
        PointerGrabAdmissionReviveCancelled) invariant=CancelledHasNoOwnership ;;
    esac
    grep -Fq "Invariant $invariant is violated." "$log" || {
        echo "TLA+ grab-admission control failed for the wrong reason: $control" >&2
        cat "$log" >&2
        exit 1
    }
done

# The content lifecycle's two controls each disable one rule a design review
# found missing, so a passing run would mean the model stopped detecting the
# defect it was written to exclude.
for control in \
    ShellContentLifecycleRetireIgnoresAssembly \
    ShellContentLifecycleSilentAssemblyTimeout; do
    control_dir="$TEMP_DIR/$control"
    mkdir "$control_dir"
    cp "$MODEL_DIR/ShellContentLifecycle.tla" "$control_dir/"
    cp "$MODEL_DIR/$control.cfg" "$control_dir/"
    log="$control_dir/control.log"
    if (
        cd "$control_dir"
        java -XX:+UseParallelGC -jar "$JAR_PATH" \
            -deadlock \
            -workers 1 \
            -fp 0 \
            -config "$control.cfg" \
            ShellContentLifecycle.tla
    ) >"$log" 2>&1; then
        echo "TLA+ negative control unexpectedly passed: $control" >&2
        exit 1
    fi
    case "$control" in
        ShellContentLifecycleRetireIgnoresAssembly)
            grep -Fq 'Invariant CandidateReferenceValidity is violated.' "$log" || {
                echo "TLA+ retire-during-assembly control failed for the wrong reason" >&2
                exit 1
            }
            ;;
        ShellContentLifecycleSilentAssemblyTimeout)
            grep -Fq 'Invariant NoAcceptedObligationLost is violated.' "$log" || {
                echo "TLA+ silent-timeout control failed for the wrong reason" >&2
                exit 1
            }
            ;;
    esac
done

# Latched readiness is the assumption neither the content nor the work-area
# model states. A passing run would mean the seam between them stopped being
# checked.
control=ShellContentBundleCompositionLatchedReadiness
control_dir="$TEMP_DIR/$control"
mkdir "$control_dir"
cp "$MODEL_DIR/ShellContentBundleComposition.tla" "$control_dir/"
cp "$MODEL_DIR/$control.cfg" "$control_dir/"
log="$control_dir/control.log"
if (
    cd "$control_dir"
    java -XX:+UseParallelGC -jar "$JAR_PATH" \
        -deadlock \
        -workers 1 \
        -fp 0 \
        -config "$control.cfg" \
        ShellContentBundleComposition.tla
) >"$log" 2>&1; then
    echo "TLA+ negative control unexpectedly passed: $control" >&2
    exit 1
fi
grep -Fq 'Invariant PresentedBundleHasLiveContent is violated.' "$log" || {
    echo "TLA+ latched-readiness control failed for the wrong reason" >&2
    exit 1
}

# A content target is input authority only after exact native presentation.
# Revocation and replacement suppress its later release without exposing that
# release to an application, while shell acknowledgement and WM admission keep
# their independent settlement boundaries.
for control in \
    ShellPresentedContentActionCapturePrepared \
    ShellPresentedContentActionRoutesRevokedRelease \
    ShellPresentedContentActionRoutesReplacedRelease \
    ShellPresentedContentActionClickThrough \
    ShellPresentedContentActionAckWaitsForWm \
    ShellPresentedContentActionAcceptsBeforeAdmission; do
    control_dir="$TEMP_DIR/$control"
    mkdir "$control_dir"
    cp "$MODEL_DIR/ShellPresentedContentAction.tla" "$control_dir/"
    cp "$MODEL_DIR/$control.cfg" "$control_dir/"
    log="$control_dir/control.log"
    if (
        cd "$control_dir"
        timeout 30m java -XX:+UseParallelGC -jar "$JAR_PATH" \
            -deadlock -workers 1 -fp 0 -config "$control.cfg" \
            ShellPresentedContentAction.tla
    ) >"$log" 2>&1; then
        echo "TLA+ shell content-action negative control unexpectedly passed: $control" >&2
        exit 1
    fi
    case "$control" in
        ShellPresentedContentActionCapturePrepared)
            invariant=CapturesNameExactPresentedContent
            ;;
        ShellPresentedContentActionRoutesRevokedRelease)
            invariant=RevokedReleaseIsSuppressed
            ;;
        ShellPresentedContentActionRoutesReplacedRelease)
            invariant=ReplacedReleaseIsSuppressed
            ;;
        ShellPresentedContentActionClickThrough)
            invariant=NoClickThrough
            ;;
        ShellPresentedContentActionAckWaitsForWm)
            invariant=AckIndependentFromWmOutcome
            ;;
        ShellPresentedContentActionAcceptsBeforeAdmission)
            invariant=AcceptedOnlyAfterWmAdmission
            ;;
    esac
    grep -Fq "Invariant $invariant is violated." "$log" || {
        echo "TLA+ shell content-action control failed for the wrong reason: $control" >&2
        cat "$log" >&2
        exit 1
    }
done

for control in InputDeliveryRecoveryNoDeadline InputDeliveryRecoveryEarlyBarrier; do
    control_dir="$TEMP_DIR/$control"
    mkdir "$control_dir"
    cp "$MODEL_DIR/InputDeliveryRecovery.tla" "$MODEL_DIR/$control.cfg" "$control_dir/"
    log="$control_dir/control.log"
    if (cd "$control_dir" && timeout 30m java -XX:+UseParallelGC -jar "$JAR_PATH" \
        -deadlock -workers 1 -fp 0 -config "$control.cfg" InputDeliveryRecovery.tla) >"$log" 2>&1; then
        echo "TLA+ input recovery negative control unexpectedly passed: $control" >&2
        exit 1
    fi
    case "$control" in
        InputDeliveryRecoveryNoDeadline) expected='Temporal properties were violated.' ;;
        InputDeliveryRecoveryEarlyBarrier) expected='Invariant BarrierSound is violated.' ;;
    esac
    grep -Fq "$expected" "$log" || { cat "$log"; exit 1; }
done
