use super::*;

impl LiveProductionNativeHead {
    /// Called only after cancellation released the prepared owner or transferred
    /// it into cleanup custody. A capacity refusal must keep these identities.
    pub(super) fn finish_prepared_cancellation(&mut self) {
        // A completed worker keeps its content in the rendering slot until KMS
        // submission or cancellation. Coalescing can cancel here through the
        // composition installer, without returning to the mirror tick.
        if self.prepared_worker_was_in_flight {
            self.rendering_content = None;
            self.output_frames.discard_rendering();
        }
        self.prepared_group_frame = None;
        self.prepared_worker_was_in_flight = false;
    }
}

#[path = "../../../../tests/support/native_prepared_cancellation.rs"]
mod tests;
