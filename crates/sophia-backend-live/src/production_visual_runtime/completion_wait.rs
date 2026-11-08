use super::*;
use crate::native_short_service as reason;

impl LiveProductionVisualRuntime {
    /// A wait view is valid only while this runtime can service the native owner.
    /// Session additionally gates it on the active seat. No fds escape the borrow.
    pub fn native_completion_wait<'a>(
        &'a self,
        native: &'a LiveProductionNativeScanout,
        request: &OutputFrameServiceRequest,
    ) -> LiveNativeCompletionWait<'a> {
        if self.native_suspended || !native.output_topology_allows_frame_service() {
            return LiveNativeCompletionWait::default();
        }
        let mut wait = native.completion_wait();
        // Singleton submissions live in the runtime; mirror submissions live
        // on native heads. Only submitted owners expose fences: retirement
        // clears them before transferring a submission to displayed custody.
        for output in self.outputs.values() {
            for (fence_output, fence) in output.runtime.completion_fences() {
                if let Some(observed) = native.completion_fence_observation(fence_output) {
                    wait.observe_fence(fence, observed);
                } else {
                    wait.short_if(true, reason::FENCE_UNOBSERVED);
                }
            }
        }
        wait.short_if(request.preparation_pending, reason::PREPARATION_PENDING);
        wait.short_if(request.presentation_queued, reason::PRESENTATION_QUEUED);
        wait.short_if(
            request.software_frame_waiting,
            reason::SOFTWARE_FRAME_WAITING,
        );
        wait.short_if(
            request
                .outputs
                .iter()
                .any(|output| output.native_phase == OutputNativeFramePhase::CleanupPending),
            reason::OUTPUT_CLEANUP_PENDING,
        );
        // A missing descriptor must never silently convert owed work to idle.
        wait.short_if(
            wait.submissions && wait.descriptors.is_empty(),
            reason::SUBMISSION_WITHOUT_DESCRIPTOR,
        );
        wait
    }
}
