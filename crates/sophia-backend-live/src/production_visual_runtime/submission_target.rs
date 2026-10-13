//! Device facts used when a native submission advances a Present cohort.
//! The runtime retains ownership classification and resource settlement.
use super::*;

pub(crate) trait NativeSubmissionTarget {
    fn submitted_frame(&self, output: OutputId) -> Option<LiveProductionNativeFrameId>;
    fn discard_presentation_feedback(&mut self, output: Option<OutputId>);
    fn activate_deferred_mirror_generation(
        &mut self,
        output: OutputId,
    ) -> Result<bool, &'static str>;
}

impl NativeSubmissionTarget for LiveProductionNativeScanout {
    fn submitted_frame(&self, output: OutputId) -> Option<LiveProductionNativeFrameId> {
        LiveProductionNativeScanout::submitted_frame(self, output)
    }

    fn discard_presentation_feedback(&mut self, output: Option<OutputId>) {
        LiveProductionNativeScanout::discard_presentation_feedback(self, output);
    }

    fn activate_deferred_mirror_generation(
        &mut self,
        output: OutputId,
    ) -> Result<bool, &'static str> {
        LiveProductionNativeScanout::activate_deferred_mirror_generation(self, output)
    }
}
