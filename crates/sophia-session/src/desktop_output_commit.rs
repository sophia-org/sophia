use sophia_backend_live::{
    LibdrmNativeAtomicCommitDevice, LibdrmNativeAtomicHead, NativeTopologySubmitIntent,
    NativeTopologySubmitOutcome, NativeTopologyValidation,
    submit_native_multi_head_topology_on_device, validate_native_complete_topology_on_device,
};

use crate::desktop_output_activation::{
    NativeOutputActivationEffectExecutor, NativeOutputActivationFailure, NativeOutputActivationKey,
    NativeOutputEffectCompletion,
};
use crate::desktop_output_topology::NativeOutputActivationPlan;

/// The heads a candidate needs, already resolved to KMS objects.
///
/// Resolving a plan into heads means naming connectors, CRTCs, planes, mode
/// blobs, and framebuffers, which is hardware state this type deliberately does
/// not reach for. The caller resolves; this carries the result. `rollback` heads
/// describe the topology to restore, which is not the inverse of `apply`:
/// restoring a previous mode needs that mode's own blobs and correctly sized
/// framebuffers.
#[derive(Clone, Debug, Default)]
pub struct NativeOutputHeadSet {
    pub apply: Vec<LibdrmNativeAtomicHead>,
    pub rollback: Vec<LibdrmNativeAtomicHead>,
}

/// Validates a candidate against real hardware and never mutates anything.
///
/// This is the executor a session runs before it owns framebuffers. Each head
/// is complete: its primary plane names a framebuffer allocated for the test
/// alone, so the kernel judges the requested desktop and not whatever a previous
/// owner left bound (amdgpu refuses an enabled CRTC with no primary plane). That
/// is the question startup and every rebuild need answered, and it is answered
/// before a single pixel is rendered.
///
/// Apply is not gated here, it is absent. The request is `TEST_ONLY`, so the
/// test buffers never reach scanout; a caller who wants to apply needs
/// `NativeOutputCommitExecutor`. Rollback succeeds trivially for the same reason:
/// nothing was applied, so nothing needs undoing.
pub struct NativeOutputTopologyValidationExecutor<'a, D, H> {
    device: &'a D,
    heads: &'a [H],
    validation: Option<NativeTopologyValidation>,
}

impl<'a, D, H> NativeOutputTopologyValidationExecutor<'a, D, H> {
    pub const fn new(device: &'a D, heads: &'a [H]) -> Self {
        Self {
            device,
            heads,
            validation: None,
        }
    }

    /// What the kernel said about the topology, as one word for evidence.
    ///
    /// The settlement alone cannot carry this. A validation that succeeds still
    /// settles as rejected, because apply then refuses, and it refuses with the
    /// same `WouldBlock` a busy device produces. Without this, an accepted topology
    /// and a busy card are indistinguishable in the log, which would make the
    /// interesting outcome invisible.
    pub const fn validation(&self) -> &'static str {
        match self.validation {
            None => "not_attempted",
            Some(NativeTopologyValidation { outcome, .. }) => match outcome {
                NativeTopologySubmitOutcome::Accepted => "accepted",
                NativeTopologySubmitOutcome::Busy => "busy",
                NativeTopologySubmitOutcome::Rejected => "rejected",
                NativeTopologySubmitOutcome::Unbuildable(_) => "unbuildable",
            },
        }
    }

    /// The kernel's errno for a busy or rejected test, zero otherwise.
    pub const fn validation_errno(&self) -> i32 {
        match self.validation {
            Some(validation) => validation.errno,
            None => 0,
        }
    }
}

impl<D, H> NativeOutputActivationEffectExecutor for NativeOutputTopologyValidationExecutor<'_, D, H>
where
    D: LibdrmNativeAtomicCommitDevice,
    H: AsRef<LibdrmNativeAtomicHead>,
{
    fn test(
        &mut self,
        _key: NativeOutputActivationKey,
        _plan: &NativeOutputActivationPlan,
    ) -> NativeOutputEffectCompletion {
        // The heads borrow buffers that outlive this call; the test reads them
        // and releases nothing.
        let heads: Vec<LibdrmNativeAtomicHead> =
            self.heads.iter().map(|head| *head.as_ref()).collect();
        let validation = validate_native_complete_topology_on_device(self.device, &heads);
        self.validation = Some(validation);
        completion(validation.outcome)
    }

    fn apply(
        &mut self,
        _key: NativeOutputActivationKey,
        _plan: &NativeOutputActivationPlan,
    ) -> NativeOutputEffectCompletion {
        // Unreachable behind a declined test, and refused if it is ever reached.
        NativeOutputEffectCompletion::Failed(NativeOutputActivationFailure::WouldBlock)
    }

    fn rollback(
        &mut self,
        _key: NativeOutputActivationKey,
        _plan: &NativeOutputActivationPlan,
    ) -> NativeOutputEffectCompletion {
        NativeOutputEffectCompletion::Succeeded
    }
}

/// Adapts topology submission to the activation reducer's effect executor.
///
/// The submission itself, including the `TEST_ONLY` validation pass and the
/// mapping from kernel result to outcome, lives in `sophia-backend-live` beside
/// the request builder and is tested there. This type only chooses which heads
/// each phase submits and translates the outcome, so the drm-facing decisions
/// stay in one place.
///
/// Apply is gated. With `apply_enabled` false the executor validates against real
/// hardware and then declines, which is the safe configuration for a session that
/// must not change output state.
pub struct NativeOutputCommitExecutor<'a, D> {
    device: &'a D,
    heads: &'a NativeOutputHeadSet,
    apply_enabled: bool,
}

impl<'a, D> NativeOutputCommitExecutor<'a, D> {
    /// Validates without ever applying.
    pub const fn validating(device: &'a D, heads: &'a NativeOutputHeadSet) -> Self {
        Self {
            device,
            heads,
            apply_enabled: false,
        }
    }

    /// Validates and then applies. Only for a caller authorized to change output
    /// state, behind its own gate and with rollback heads populated.
    pub const fn activating(device: &'a D, heads: &'a NativeOutputHeadSet) -> Self {
        Self {
            device,
            heads,
            apply_enabled: true,
        }
    }
}

#[cfg(feature = "native-session")]
const fn completion(outcome: NativeTopologySubmitOutcome) -> NativeOutputEffectCompletion {
    match outcome {
        NativeTopologySubmitOutcome::Accepted => NativeOutputEffectCompletion::Succeeded,
        NativeTopologySubmitOutcome::Busy => {
            NativeOutputEffectCompletion::Failed(NativeOutputActivationFailure::WouldBlock)
        }
        // An unbuildable head set and a kernel refusal both mean this candidate
        // cannot be activated. The reducer discards either the same way.
        NativeTopologySubmitOutcome::Rejected | NativeTopologySubmitOutcome::Unbuildable(_) => {
            NativeOutputEffectCompletion::Failed(NativeOutputActivationFailure::Rejected)
        }
    }
}

impl<D> NativeOutputActivationEffectExecutor for NativeOutputCommitExecutor<'_, D>
where
    D: LibdrmNativeAtomicCommitDevice,
{
    fn test(
        &mut self,
        _key: NativeOutputActivationKey,
        _plan: &NativeOutputActivationPlan,
    ) -> NativeOutputEffectCompletion {
        completion(submit_native_multi_head_topology_on_device(
            self.device,
            &self.heads.apply,
            NativeTopologySubmitIntent::Validate,
        ))
    }

    fn apply(
        &mut self,
        _key: NativeOutputActivationKey,
        _plan: &NativeOutputActivationPlan,
    ) -> NativeOutputEffectCompletion {
        if !self.apply_enabled {
            return NativeOutputEffectCompletion::Failed(NativeOutputActivationFailure::WouldBlock);
        }
        completion(submit_native_multi_head_topology_on_device(
            self.device,
            &self.heads.apply,
            NativeTopologySubmitIntent::Activate,
        ))
    }

    fn rollback(
        &mut self,
        _key: NativeOutputActivationKey,
        _plan: &NativeOutputActivationPlan,
    ) -> NativeOutputEffectCompletion {
        // Restoring nothing is a successful restore: an apply that never reached
        // the kernel left no state to undo.
        if self.heads.rollback.is_empty() {
            return NativeOutputEffectCompletion::Succeeded;
        }
        completion(submit_native_multi_head_topology_on_device(
            self.device,
            &self.heads.rollback,
            NativeTopologySubmitIntent::Activate,
        ))
    }
}
