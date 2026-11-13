//! The native half the Session's content path drives (GATE3-SCOPE G3-5a).
//! One sealed trait, implemented by the production owner and by the
//! device-free combined fixture through the same runtime entries and the
//! same combined routes. The concrete public setters stay as they are.
use super::*;
#[cfg(feature = "experimental-combined-frame-job")]
use crate::{
    CombinedAccept, CombinedComposeRefusal, CombinedComposition, CombinedDispatch, CombinedHead,
    CombinedRecompose, CombinedRelease, CombinedRenderPlan, CombinedRenderResult,
    CombinedSettleRefused, LiveProductionNativeFrameId,
};
#[cfg(feature = "experimental-combined-frame-job")]
use sophia_runtime::{CombinedInbox, CombinedReservation, IsbValidationWorker, ValidatedIsbPixels};

mod sealed {
    pub trait Sealed {
        /// The compose result boundary (G3-5b Q1). Production passes the
        /// result through; the fixture may declare a fault here.
        #[cfg(feature = "experimental-combined-frame-job")]
        fn compose_result(
            &self,
            composed: Result<crate::CombinedComposition, crate::CombinedComposeRefusal>,
        ) -> Result<crate::CombinedComposition, crate::CombinedComposeRefusal> {
            composed
        }

        /// The recomposition's classification at consumption (G3-5c).
        /// Production passes it through; the fixture may declare an
        /// unclassified fault here.
        #[cfg(feature = "experimental-combined-frame-job")]
        fn classify_result(
            &self,
            classified: Result<crate::CombinedRecompose, Box<dyn std::error::Error>>,
        ) -> Result<crate::CombinedRecompose, Box<dyn std::error::Error>> {
            classified
        }
    }
}

/// Each method names the Session caller it exists for.
pub trait ShellContentNative: sealed::Sealed {
    /// `set_shell_component_content` on this native. Caller: Session's
    /// `submit_bundle`, ordinary arm.
    fn install_shell_content(
        &mut self,
        runtime: &mut LiveProductionVisualRuntime,
        frame: LiveShellContentFrame,
        layer: LiveShellContentLayer,
        scene: &LiveProductionCpuScene,
    ) -> Result<bool, Box<dyn std::error::Error>>;

    /// `withdraw_shell_popout` on this native. Caller: Session's dismissal
    /// service, which the content service forwards its native to.
    fn withdraw_shell_popout(
        &mut self,
        runtime: &mut LiveProductionVisualRuntime,
        grant: ContentGrant,
        output: ContentOutputId,
        allocation: ContentAllocationId,
        scene: &LiveProductionCpuScene,
    ) -> Result<bool, Box<dyn std::error::Error>>;

    /// One composition of `candidate` at `layer` for a combined job, with
    /// this native's owner facts and the turn's one cursor snapshot.
    /// Caller: the row's `reserve` hook, before any reservation.
    #[cfg(feature = "experimental-combined-frame-job")]
    fn compose_combined_frame(
        &self,
        runtime: &LiveProductionVisualRuntime,
        scene: &LiveProductionCpuScene,
        layer: LiveShellContentLayer,
        candidate: &LiveShellContentFrame,
        cursor: crate::LiveProductionCursorPresentation,
        plane: Option<sophia_protocol::Point>,
    ) -> Result<CombinedComposition, CombinedComposeRefusal>;

    /// The plan for one admitted copy of `composition`, as `frame`, under
    /// `head`'s reservation; `None` is a refused send. Caller: the row's
    /// `plan` hook.
    #[cfg(feature = "experimental-combined-frame-job")]
    fn plan_combined_frame(
        &mut self,
        composition: CombinedComposition,
        head: &CombinedHead,
        frame: LiveProductionNativeFrameId,
        admitted: &sophia_protocol::ContentResourceBegin,
        deadline: std::time::Instant,
    ) -> Option<(CombinedRenderPlan, CombinedDispatch)>;

    /// The native frame a combined admission is planned as, from the
    /// counter ordinary batches draw from. Caller: the row's `plan` hook.
    #[cfg(feature = "experimental-combined-frame-job")]
    fn allocate_combined_frame(&mut self) -> LiveProductionNativeFrameId;

    /// Caller: the row's `reserve` hook.
    #[cfg(feature = "experimental-combined-frame-job")]
    fn reserve_combined_frame(
        &mut self,
        output: OutputId,
    ) -> std::io::Result<(CombinedHead, CombinedInbox<CombinedRenderPlan>)>;

    /// Caller: the row's `cancel` hook.
    #[cfg(feature = "experimental-combined-frame-job")]
    fn cancel_combined_frame(&mut self, head: &CombinedHead) -> std::io::Result<bool>;

    /// Caller: the row's `collect` hook.
    #[cfg(feature = "experimental-combined-frame-job")]
    fn take_combined_frame_result(
        &mut self,
        head: &CombinedHead,
    ) -> std::io::Result<Option<CombinedRenderResult>>;

    /// Caller: the row's `collect` hook.
    #[cfg(feature = "experimental-combined-frame-job")]
    fn settle_combined_frame(
        &mut self,
        head: &CombinedHead,
        validator: &mut IsbValidationWorker<CombinedRenderPlan>,
        reservation: CombinedReservation,
        result: CombinedRenderResult,
    ) -> Result<ValidatedIsbPixels, CombinedSettleRefused>;

    /// Caller: the row walk, for a declined Held result.
    #[cfg(feature = "experimental-combined-frame-job")]
    fn release_combined_held(&mut self, head: &CombinedHead, job: u64) -> CombinedRelease;

    /// Caller: the row walk's Release-debt maintenance.
    #[cfg(feature = "experimental-combined-frame-job")]
    fn flush_combined_release(&mut self, head: &CombinedHead, job: u64) -> CombinedRelease;

    /// The accept's read-only checks before its presentation, in its
    /// order (G3-5c C1). Caller: Session's `submit_bundle`, combined arm,
    /// before it classifies, so a scene change never excuses a refusal.
    #[cfg(feature = "experimental-combined-frame-job")]
    fn check_combined_consumption(
        &self,
        runtime: &LiveProductionVisualRuntime,
        frame: &LiveShellContentFrame,
        layer: LiveShellContentLayer,
        head: &CombinedHead,
        accept: &CombinedAccept,
    ) -> Result<(), compositor_graphics::CombinedAcceptError>;

    /// Recomposes `candidate` at `layer` with this native's facts and the
    /// turn's cursor, and classifies it against the dispatch key (F1, R4).
    /// An unclassified composition failure comes back as the error.
    /// Caller: Session's `submit_bundle`, combined arm.
    #[cfg(feature = "experimental-combined-frame-job")]
    #[allow(clippy::too_many_arguments)]
    fn classify_combined_frame(
        &self,
        runtime: &LiveProductionVisualRuntime,
        scene: &LiveProductionCpuScene,
        layer: LiveShellContentLayer,
        candidate: &LiveShellContentFrame,
        cursor: crate::LiveProductionCursorPresentation,
        plane: Option<sophia_protocol::Point>,
        dispatch: &CombinedDispatch,
    ) -> Result<CombinedRecompose, Box<dyn std::error::Error>>;

    /// The two-phase accept (G3-4). Caller: Session's `submit_bundle`,
    /// combined arm.
    #[cfg(feature = "experimental-combined-frame-job")]
    fn accept_combined_shell_content(
        &mut self,
        runtime: &mut LiveProductionVisualRuntime,
        frame: LiveShellContentFrame,
        layer: LiveShellContentLayer,
        head: &CombinedHead,
        accept: CombinedAccept,
    ) -> Result<(), Box<compositor_graphics::CombinedAcceptFailed>>;
}

/// One body for every implementation: the runtime's own target entries with
/// this native as the target, and the type's inherent combined entries.
macro_rules! shell_content_native {
    ($native:ty) => {
        impl ShellContentNative for $native {
            fn install_shell_content(
                &mut self,
                runtime: &mut LiveProductionVisualRuntime,
                frame: LiveShellContentFrame,
                layer: LiveShellContentLayer,
                scene: &LiveProductionCpuScene,
            ) -> Result<bool, Box<dyn std::error::Error>> {
                runtime.set_shell_component_content_on_target(frame, layer, scene, Some(self))
            }

            fn withdraw_shell_popout(
                &mut self,
                runtime: &mut LiveProductionVisualRuntime,
                grant: ContentGrant,
                output: ContentOutputId,
                allocation: ContentAllocationId,
                scene: &LiveProductionCpuScene,
            ) -> Result<bool, Box<dyn std::error::Error>> {
                runtime.withdraw_shell_popout_on_target(
                    grant,
                    output,
                    allocation,
                    scene,
                    Some(self),
                )
            }

            #[cfg(feature = "experimental-combined-frame-job")]
            fn compose_combined_frame(
                &self,
                runtime: &LiveProductionVisualRuntime,
                scene: &LiveProductionCpuScene,
                layer: LiveShellContentLayer,
                candidate: &LiveShellContentFrame,
                cursor: crate::LiveProductionCursorPresentation,
                plane: Option<sophia_protocol::Point>,
            ) -> Result<CombinedComposition, CombinedComposeRefusal> {
                let owner = <$native>::combined_owner_state(self, candidate.output);
                let cursor =
                    compositor_graphics::combined_scene::CombinedCursor::new(cursor, plane);
                let composed =
                    runtime.compose_combined_frame(self, owner, scene, layer, candidate, cursor);
                sealed::Sealed::compose_result(self, composed)
            }

            #[cfg(feature = "experimental-combined-frame-job")]
            fn plan_combined_frame(
                &mut self,
                composition: CombinedComposition,
                head: &CombinedHead,
                frame: LiveProductionNativeFrameId,
                admitted: &sophia_protocol::ContentResourceBegin,
                deadline: std::time::Instant,
            ) -> Option<(CombinedRenderPlan, CombinedDispatch)> {
                <$native>::plan_combined_frame(self, composition, head, frame, admitted, deadline)
            }

            #[cfg(feature = "experimental-combined-frame-job")]
            fn allocate_combined_frame(&mut self) -> LiveProductionNativeFrameId {
                <$native>::allocate_combined_frame(self)
            }

            #[cfg(feature = "experimental-combined-frame-job")]
            fn reserve_combined_frame(
                &mut self,
                output: OutputId,
            ) -> std::io::Result<(CombinedHead, CombinedInbox<CombinedRenderPlan>)> {
                <$native>::reserve_combined_frame(self, output)
            }

            #[cfg(feature = "experimental-combined-frame-job")]
            fn cancel_combined_frame(&mut self, head: &CombinedHead) -> std::io::Result<bool> {
                <$native>::cancel_combined_frame(self, head)
            }

            #[cfg(feature = "experimental-combined-frame-job")]
            fn take_combined_frame_result(
                &mut self,
                head: &CombinedHead,
            ) -> std::io::Result<Option<CombinedRenderResult>> {
                <$native>::take_combined_frame_result(self, head)
            }

            #[cfg(feature = "experimental-combined-frame-job")]
            fn settle_combined_frame(
                &mut self,
                head: &CombinedHead,
                validator: &mut IsbValidationWorker<CombinedRenderPlan>,
                reservation: CombinedReservation,
                result: CombinedRenderResult,
            ) -> Result<ValidatedIsbPixels, CombinedSettleRefused> {
                <$native>::settle_combined_frame(self, head, validator, reservation, result)
            }

            #[cfg(feature = "experimental-combined-frame-job")]
            fn release_combined_held(&mut self, head: &CombinedHead, job: u64) -> CombinedRelease {
                <$native>::release_combined_held(self, head, job)
            }

            #[cfg(feature = "experimental-combined-frame-job")]
            fn flush_combined_release(&mut self, head: &CombinedHead, job: u64) -> CombinedRelease {
                <$native>::flush_combined_release(self, head, job)
            }

            #[cfg(feature = "experimental-combined-frame-job")]
            fn check_combined_consumption(
                &self,
                runtime: &LiveProductionVisualRuntime,
                frame: &LiveShellContentFrame,
                layer: LiveShellContentLayer,
                head: &CombinedHead,
                accept: &CombinedAccept,
            ) -> Result<(), compositor_graphics::CombinedAcceptError> {
                runtime
                    .check_combined_consumption(frame, layer, self, head, accept)
                    .map(|_| ())
            }

            #[cfg(feature = "experimental-combined-frame-job")]
            fn classify_combined_frame(
                &self,
                runtime: &LiveProductionVisualRuntime,
                scene: &LiveProductionCpuScene,
                layer: LiveShellContentLayer,
                candidate: &LiveShellContentFrame,
                cursor: crate::LiveProductionCursorPresentation,
                plane: Option<sophia_protocol::Point>,
                dispatch: &CombinedDispatch,
            ) -> Result<CombinedRecompose, Box<dyn std::error::Error>> {
                let cursor =
                    compositor_graphics::combined_scene::CombinedCursor::new(cursor, plane);
                let current =
                    runtime.compose_provisional_head_frames(self, scene, layer, candidate, cursor);
                let classified = dispatch.key.classify(current, &dispatch.candidate);
                sealed::Sealed::classify_result(self, classified)
            }

            #[cfg(feature = "experimental-combined-frame-job")]
            fn accept_combined_shell_content(
                &mut self,
                runtime: &mut LiveProductionVisualRuntime,
                frame: LiveShellContentFrame,
                layer: LiveShellContentLayer,
                head: &CombinedHead,
                accept: CombinedAccept,
            ) -> Result<(), Box<compositor_graphics::CombinedAcceptFailed>> {
                runtime.accept_combined_shell_content(frame, layer, self, head, accept)
            }
        }
    };
}

impl sealed::Sealed for LiveProductionNativeScanout {}
shell_content_native!(LiveProductionNativeScanout);

#[cfg(all(
    any(test, feature = "test-support"),
    feature = "experimental-combined-frame-job"
))]
impl sealed::Sealed for crate::CombinedNativeFixture {
    /// Counts each composition; a declared fault replaces its result with
    /// an unclassified error after the real composition ran.
    fn compose_result(
        &self,
        composed: Result<CombinedComposition, CombinedComposeRefusal>,
    ) -> Result<CombinedComposition, CombinedComposeRefusal> {
        self.composes.set(self.composes.get() + 1);
        match self.compose_fault {
            Some(reason) => Err(CombinedComposeRefusal::Unclassified(reason.into())),
            None => composed,
        }
    }
    /// Test-only fault seam (G3-5c controls 3 to 5): counts each
    /// classification; a declared fault replaces its answer with an
    /// unclassified error, and a declared class replaces a classified
    /// answer, both after the real recomposition ran.
    fn classify_result(
        &self,
        classified: Result<CombinedRecompose, Box<dyn std::error::Error>>,
    ) -> Result<CombinedRecompose, Box<dyn std::error::Error>> {
        self.classifies.set(self.classifies.get() + 1);
        match (self.classify_fault, classified) {
            (Some(reason), _) => Err(reason.into()),
            (None, Ok(_)) if self.classification.is_some() => {
                Ok(self.classification.expect("checked"))
            }
            (None, classified) => classified,
        }
    }
}
#[cfg(all(
    any(test, feature = "test-support"),
    feature = "experimental-combined-frame-job"
))]
shell_content_native!(crate::CombinedNativeFixture);
