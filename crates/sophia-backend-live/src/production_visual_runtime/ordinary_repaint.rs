//! Replaceable repaint obligations are output-local metadata, not pixel owners.
use super::*;

/// An absent ID means deferred, never presented. Invalid batches remain errors.
pub(super) fn admit<T: NativeCompositionTarget>(
    pending: &mut BTreeSet<OutputId>,
    native: &mut T,
    output: OutputId,
    frames: Vec<crate::LiveProductionHeadCompositionFrame>,
) -> Result<Option<crate::LiveProductionNativeFrameId>, Box<dyn std::error::Error>> {
    let queued = native.queue_ordinary_batch(vec![(output, frames)])?;
    let frame = queued.get(&output).copied();
    if frame.is_some() {
        pending.remove(&output);
    } else {
        pending.insert(output);
    }
    Ok(frame)
}

impl LiveProductionVisualRuntime {
    pub(super) fn service_ordinary_repaints<T: NativeCompositionTarget>(
        &mut self,
        scene: &LiveProductionCpuScene,
        native: &mut T,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self.ordinary_repaints_pending.is_empty()
            || self.native_publication_blocked()
            || !native.frame_service_available()
        {
            return Ok(());
        }
        let ready = self
            .ordinary_repaints_pending
            .iter()
            .copied()
            .filter(|output| native.required_outputs_ready(&BTreeSet::from([*output])))
            .collect::<Vec<_>>();
        if ready.is_empty() {
            return Ok(());
        }
        let sources = self.retained_composition_source_set(scene, self.in_flight_direct(native))?;
        for output in ready {
            let viewport = self
                .outputs
                .logical_viewport(output)
                .ok_or("repaint targets an absent output")?;
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
                sources.scene_generation,
                &sources.sources,
            )?;
            admit(&mut self.ordinary_repaints_pending, native, output, frames)?;
        }
        Ok(())
    }
}
