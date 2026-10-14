//! An authorized capture requests a fresh retained composition. It never
//! treats a scene counter or an unretired render result as presentation.
use super::*;

impl LiveProductionVisualRuntime {
    pub fn queue_output_capture(
        &mut self,
        native: &mut LiveProductionNativeScanout,
        scene: &LiveProductionCpuScene,
        output: OutputId,
    ) -> Result<Option<crate::LiveProductionNativeFrameId>, Box<dyn std::error::Error>> {
        if self.native_publication_blocked()
            || self.present_scheduler.has_eligible()
            || !native.output_topology_allows_frame_service()
            || !native.frame_queue_ready(output)
            || native.pending_frame(output)
        {
            return Ok(None);
        }
        let Some(viewport) = self.outputs.logical_viewport(output) else {
            return Ok(None);
        };
        let sources = self.retained_composition_source_set(scene, self.in_flight_direct(native))?;
        let list = self.display_list_for_output(
            output,
            viewport,
            &sources.committed,
            &sources.presentation_order,
        )?;
        let frames = self.compose_native_head_frames_from_sources(
            native,
            output,
            &sources.committed,
            list,
            (sources.scene_generation, None),
            &sources.sources,
        )?;
        let queued = native.queue_ordinary_head_composition_batch(vec![(output, frames)])?;
        let Some(frame) = queued.get(&output).copied() else {
            return Ok(None);
        };
        // No worker service occurs between installation and this exact-frame
        // request. A successor cannot inherit it if normal rendering overtakes it.
        Ok(native
            .request_pending_output_capture(output)
            .then_some(frame))
    }

    pub fn presented_output_capture(
        &self,
        native: &LiveProductionNativeScanout,
        output: OutputId,
    ) -> Option<crate::LiveNativeFrameCapture> {
        native.presented_output_capture(output, &self.outputs)
    }
}
