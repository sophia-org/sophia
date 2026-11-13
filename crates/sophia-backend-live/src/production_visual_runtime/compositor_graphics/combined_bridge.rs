//! G3-5b: the bridge Session's combined row drives, opaque both ways. The
//! row composes once before any reservation and plans only an admitted copy
//! of exactly the composed candidate. Lowering, keys and the plan stay in
//! the backend; Session holds owned values it cannot open.
use super::*;

/// A candidate composed for the combined job and found eligible, before any
/// reservation. Only the native owner's plan consumes it.
pub struct CombinedComposition {
    pub(crate) output: OutputId,
    pub(crate) frame: LiveProvisionalHeadFrame,
    pub(crate) key: CombinedSceneKey,
    pub(crate) candidate: Vec<CompositorImageSourceIdentity>,
}

impl CombinedComposition {
    pub fn output(&self) -> OutputId {
        self.output
    }
}

/// Why no composition came back.
#[derive(Debug)]
pub enum CombinedComposeRefusal {
    /// The conventional path takes this candidate.
    Ineligible(CombinedIneligible),
    /// The output has no viewport or head, or the planner did not cover its
    /// head targets; the conventional path takes it.
    Topology,
    /// Any other composition failure, unclassified: the first error, for the
    /// caller to fail closed with. Never retried, never taken as a decline.
    Unclassified(Box<dyn std::error::Error>),
}

/// What a planned admission keeps until its consumption: the scene key and
/// candidate a recomposition is classified against, and the accept's facts.
pub struct CombinedDispatch {
    pub(crate) frame: crate::LiveProductionNativeFrameId,
    pub(crate) key: CombinedSceneKey,
    pub(crate) candidate: Vec<CompositorImageSourceIdentity>,
    pub(crate) checksum: u64,
    pub(crate) snapshot: sophia_engine::OutputFrameDamageSnapshot,
    pub(crate) correlation: crate::LiveRendererFrameCorrelation,
    pub(crate) output_format: Option<sophia_renderer_live::LiveCompositionFormatRequest>,
}

impl CombinedDispatch {
    pub fn frame(&self) -> crate::LiveProductionNativeFrameId {
        self.frame
    }

    /// The accept for runtime job `job`, from the plan as it was dispatched.
    pub fn accept(&self, job: u64) -> crate::CombinedAccept {
        crate::CombinedAccept {
            job,
            frame: self.frame,
            checksum: self.checksum,
            snapshot: self.snapshot.clone(),
            correlation: self.correlation,
            output_format: self.output_format,
        }
    }
}

impl LiveProductionVisualRuntime {
    /// Service 1's composition: unknown owner facts decline before anything
    /// composes; then one composition, whose unclassified error comes back
    /// as it is; then eligibility over that frame and the owner's facts.
    pub(crate) fn compose_combined_frame<T: NativeCompositionTarget>(
        &self,
        native: &T,
        owner: Option<CombinedOwnerState>,
        scene: &LiveProductionCpuScene,
        layer: LiveShellContentLayer,
        candidate: &LiveShellContentFrame,
        cursor: CombinedCursor,
    ) -> Result<CombinedComposition, CombinedComposeRefusal> {
        let owner = owner.ok_or(CombinedComposeRefusal::Ineligible(
            CombinedIneligible::OwnerFactsUnknown,
        ))?;
        let mut frames = self
            .compose_provisional_head_frames(native, scene, layer, candidate, cursor)
            .map_err(|error| match error {
                ProvisionalComposeError::Topology => CombinedComposeRefusal::Topology,
                ProvisionalComposeError::Composition(error) => {
                    CombinedComposeRefusal::Unclassified(error)
                }
            })?;
        combined_eligibility(&frames, candidate, owner)
            .map_err(CombinedComposeRefusal::Ineligible)?;
        let frame = frames.pop().expect("eligibility admits exactly one head");
        let ids = candidate
            .images
            .iter()
            .map(|image| image.resource.identity())
            .collect::<Vec<_>>();
        let key = CombinedSceneKey::of(&frame, &ids);
        Ok(CombinedComposition {
            output: candidate.output,
            frame,
            key,
            candidate: ids,
        })
    }
}
