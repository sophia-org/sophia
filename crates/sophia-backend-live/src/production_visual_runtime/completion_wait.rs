use super::*;

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
                    wait.short_service = true;
                }
            }
        }
        wait.short_service |= request.preparation_pending
            || request.presentation_queued
            || request.software_frame_waiting
            || request
                .outputs
                .iter()
                .any(|output| output.native_phase == OutputNativeFramePhase::CleanupPending);
        // A missing descriptor must never silently convert owed work to idle.
        wait.short_service |= wait.submissions && wait.descriptors.is_empty();
        wait
    }
}
