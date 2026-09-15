//! Physical owner retirement and logical mirror progress share one completion.
//! The caller supplies current native authority; payload identity is never authority.
use super::*;

pub(crate) struct MirrorCompletionWitness {
    pub expected: crate::LiveNativeFrameIdentity,
    pub callback: crate::LivePageFlipCallback,
    pub last_callback_serial: Option<u64>,
    pub ust_usec: u64,
}

pub(crate) struct MirrorCompletionResult {
    pub physical: crate::PersistentFlipOutcome,
    pub cohort: Option<sophia_engine::OutputPresentationTransition>,
    pub logical: Option<LiveProductionMirrorHeadTransition>,
    pub timing_valid: bool,
}

pub(crate) fn complete_mirror_head<D: crate::LibdrmNativePrimaryPlaneResourceDevice>(
    device: &D,
    custody: &mut crate::PersistentScanoutCustody,
    lifecycle: &mut LiveProductionMirrorGroupLifecycle,
    cohort: Option<&mut sophia_engine::OutputPresentationCohort>,
    witness: MirrorCompletionWitness,
) -> MirrorCompletionResult {
    let MirrorCompletionWitness {
        expected,
        callback,
        last_callback_serial,
        ust_usec,
    } = witness;
    let mut result = MirrorCompletionResult {
        physical: crate::PersistentFlipOutcome::IdentityMismatch,
        cohort: None,
        logical: None,
        timing_valid: true,
    };
    let frame = LiveProductionNativeFrameId::from_raw(expected.frame());
    if callback.output != expected.output()
        || callback.head != expected.head()
        || lifecycle.output() != expected.output()
        || lifecycle.submitted_frame(callback.head) != Some(frame)
        || cohort.as_ref().is_some_and(|cohort| {
            cohort.output() != expected.output()
                || cohort.scene_generation() != expected.frame()
                || cohort.primary_head() != lifecycle.primary_head()
                || !cohort.required_heads().eq(lifecycle.heads())
                || !cohort.head_awaits_flip(expected.head())
                || cohort
                    .prepared_candidate(expected.head())
                    .is_none_or(|candidate| {
                        candidate.output != expected.output()
                            || candidate.scene_generation != expected.frame()
                            || candidate.head != expected.head()
                            || candidate.target_generation != expected.target_generation()
                    })
        })
    {
        return result;
    }
    if last_callback_serial.is_some_and(|serial| callback.frame_serial <= serial) {
        result.physical = crate::PersistentFlipOutcome::Waiting;
        return result;
    }
    result.physical = custody.present(
        device,
        &crate::LivePageFlipCallbackReport {
            decision: crate::LivePageFlipCallbackDecision::Accepted,
            event: crate::LivePageFlipEvent {
                status: crate::LivePageFlipEventStatus::Presented,
                frame_serial: Some(callback.frame_serial),
            },
        },
        Some(expected),
    );
    if !matches!(
        result.physical,
        crate::PersistentFlipOutcome::Presented { .. }
    ) {
        return result;
    }
    // A failed predecessor cleanup cannot retract the new physical presentation.
    result.cohort = cohort.map(|cohort| {
        if lifecycle.failed() {
            // An older in-flight cohort may not have been the cohort which
            // poisoned the group. Record its physical drain without minting
            // a fresh logical success. fail() preserves any existing terminal.
            cohort.fail(sophia_engine::OutputPresentationFailure::Invariant);
        }
        cohort.mark_flipped(callback.head, ust_usec)
    });
    if !lifecycle.failed() {
        result.timing_valid =
            lifecycle.observe_flip_timing(callback.head, frame, callback.frame_serial, ust_usec);
        if result.timing_valid {
            result.logical = Some(lifecycle.mark_flipped(callback.head, frame));
        }
    }
    result
}
