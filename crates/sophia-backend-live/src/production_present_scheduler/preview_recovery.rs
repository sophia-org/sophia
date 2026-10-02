use super::*;

impl LiveProductionPresentScheduler {
    /// Replace only a frame that never earned a submission. The transaction's
    /// required outputs, clock and every submitted/retired owner stay fixed.
    pub fn replace_unsubmitted_frame(
        &mut self,
        output: sophia_protocol::OutputId,
        old: LiveProductionNativeFrameId,
        new: LiveProductionNativeFrameId,
    ) -> bool {
        if self.was_submitted(output, old) {
            return false;
        }
        let Some(LiveProductionInFlightPresent::Rendering(present)) = self.in_flight.as_mut()
        else {
            return false;
        };
        if present.output_cohort.output_submitted(output)
            || present.frames.get(&output) != Some(&old)
            || new <= old
            || present.frames.values().any(|frame| *frame == new)
        {
            return false;
        }
        present.frames.insert(output, new);
        true
    }
}
