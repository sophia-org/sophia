//! Where retained renderer images go across a native owner replacement, and
//! which surfaces have no drawable source meanwhile (t306).

use super::*;

impl LiveProductionVisualRuntime {
    /// After a forced revocation no store can be exported, so the retained
    /// images are gone. Each surface that drew from one is marked lost
    /// everywhere rather than left committed with no source, which the next
    /// composition would refuse; its next committed Present brings it back.
    /// Images already held only as pending snapshots keep them.
    pub fn discard_retained_renderer_images(&mut self) -> usize {
        let discarded = self.displayed_surfaces.len();
        let pending = self
            .pending_renderer_handoff
            .as_ref()
            .map(|handoff| handoff.image_ids().iter().copied().collect::<BTreeSet<_>>())
            .unwrap_or_default();
        let lost = self
            .displayed_surfaces
            .iter()
            .filter(|(_, displayed)| !pending.contains(&displayed.layer.image_id))
            .map(|(surface, _)| *surface)
            .collect::<Vec<_>>();
        for surface in &lost {
            self.source_availability
                .mark(*surface, crate::LiveSourceUnavailableReason::Lost, None);
        }
        if !lost.is_empty() {
            self.revoke_policy_presentation_for_removed(&lost);
        }
        // A surface whose image survives as a pending snapshot keeps its
        // link to that image, which a later placement draws from.
        self.displayed_surfaces
            .retain(|_, displayed| pending.contains(&displayed.layer.image_id));
        discarded
    }

    /// The snapshots of retained images no store holds, for the next handoff
    /// to absorb. Images no longer retained are dropped with their surfaces.
    pub fn take_pending_renderer_handoff(&mut self) -> Option<LiveProductionRendererImageHandoff> {
        self.prune_pending_renderer_handoff();
        self.pending_renderer_retry.clear();
        self.pending_renderer_handoff.take()
    }

    /// Drops pending snapshots of images no surface retains any more, at any
    /// time, gate or not; an empty residual and its gate go with them.
    fn prune_pending_renderer_handoff(&mut self) {
        let Some(handoff) = self.pending_renderer_handoff.as_mut() else {
            return;
        };
        let retained = self
            .displayed_surfaces
            .values()
            .map(|displayed| displayed.layer.image_id)
            .collect::<BTreeSet<_>>();
        handoff.retain_only(&retained);
        if handoff.is_empty() {
            self.pending_renderer_handoff = None;
            self.pending_renderer_retry.clear();
        }
    }

    /// Returns snapshots taken for a handoff whose export then failed.
    pub fn return_pending_renderer_handoff(
        &mut self,
        handoff: Option<LiveProductionRendererImageHandoff>,
    ) {
        if self.pending_renderer_handoff.is_none() {
            self.pending_renderer_handoff = handoff;
        }
    }

    /// Keeps the snapshots of images a resume left without a store, after the
    /// replacement published. `progress` is the owner's storage progress then
    /// and `busy` whether a deferral waited on GPU work, so the first retry
    /// waits for the right change.
    pub fn keep_pending_renderer_handoff(
        &mut self,
        handoff: LiveProductionRendererImageHandoff,
        progress: u64,
        busy: bool,
    ) {
        if !handoff.is_empty() {
            self.pending_renderer_handoff = Some(handoff);
            self.pending_renderer_retry.observe(progress, busy);
        }
    }

    /// Whether pending images are due another offer: the owner then services
    /// native work on its short pacing instead of idling.
    pub fn pending_renderer_images_due(
        &self,
        native_scanout: &LiveProductionNativeScanout,
    ) -> bool {
        self.pending_renderer_handoff.is_some()
            && self
                .pending_renderer_retry
                .due(native_scanout.renderer_storage_progress())
    }

    /// Offers pending snapshots to the stores again when the retry gate says
    /// something may have changed (REVIEW-CODEX-06 R1).
    pub fn place_pending_renderer_images(
        &mut self,
        native_scanout: &mut LiveProductionNativeScanout,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.prune_pending_renderer_handoff();
        if !self.pending_renderer_images_due(native_scanout) {
            return Ok(());
        }
        let Some(mut handoff) = self.pending_renderer_handoff.take() else {
            return Ok(());
        };
        let restore = match native_scanout.place_pending_renderer_images(&handoff) {
            Ok(restore) => restore,
            Err(error) => {
                self.pending_renderer_handoff = Some(handoff);
                return Err(error);
            }
        };
        self.pending_renderer_retry
            .observe(native_scanout.renderer_storage_progress(), restore.busy);
        for image in &restore.restored {
            if !self.source_availability.image_placed(*image).is_empty() {
                self.ordinary_repaints_pending
                    .extend(self.outputs.logical_viewports().map(|(output, _)| output));
            }
        }
        handoff.retain_only(&restore.pending);
        if handoff.is_empty() {
            self.pending_renderer_retry.clear();
        } else {
            self.pending_renderer_handoff = Some(handoff);
        }
        if !restore.restored.is_empty() {
            tracing::info!(
                "sophia_live_renderer_image_handoff schema=2 status=pending_placed placed_images={} pending_images={}",
                restore.restored.len(),
                restore.pending.len(),
            );
        }
        Ok(())
    }

    /// The output's stores refused an image a surface there samples; another
    /// store keeps it. Those surfaces are left out on that output only, until
    /// their next committed Present.
    pub(in crate::production_visual_runtime) fn mark_cold_refusals(
        &mut self,
        refused: &[(LiveRendererImageId, OutputId)],
    ) {
        if refused.is_empty() {
            return;
        }
        let mut marked = Vec::new();
        for (surface, displayed) in &self.displayed_surfaces {
            let outputs = refused
                .iter()
                .filter(|(image, _)| *image == displayed.layer.image_id)
                .map(|(_, output)| *output)
                .collect::<BTreeSet<_>>();
            if !outputs.is_empty() {
                tracing::warn!(
                    "sophia_live_source_availability schema=1 status=unavailable surface={} image={} outputs={} reason=import_refused scope=outputs",
                    surface.index(),
                    displayed.layer.image_id.raw(),
                    outputs.len(),
                );
                self.source_availability.mark(
                    *surface,
                    crate::LiveSourceUnavailableReason::Lost,
                    Some(outputs),
                );
                marked.push(*surface);
            }
        }
        if !marked.is_empty() {
            self.revoke_policy_presentation_for_removed(&marked);
        }
    }

    /// Per output of the replacement, the retained images its first frames
    /// sample, read from display lists over the replacement's viewports while
    /// the runtime is still suspended: nothing is published and no image read
    /// is taken. A surface still on a vanished output samples nothing here.
    pub(in crate::production_visual_runtime) fn retained_image_demand(
        &self,
        outputs: &LiveProductionOutputRuntimeSet,
    ) -> Result<BTreeMap<OutputId, BTreeSet<LiveRendererImageId>>, Box<dyn std::error::Error>> {
        let committed = self.displayed_surface_view().to_vec();
        let order = live_production_retained_surface_order(&self.presentation_order, &committed);
        let mut demand = BTreeMap::new();
        for (output, viewport) in outputs.logical_viewports() {
            let list = self.display_list_for_output(output, viewport, &committed, &order)?;
            let images = list
                .commands
                .iter()
                .filter_map(|command| match command {
                    CompositorDisplayCommand::Surface { surface } => Some(surface),
                    CompositorDisplayCommand::SurfaceInstance(instance) => Some(&instance.source),
                    _ => None,
                })
                .filter_map(|surface| self.displayed_surfaces.get(surface))
                .map(|displayed| displayed.layer.image_id)
                .collect::<BTreeSet<_>>();
            demand.insert(output, images);
        }
        Ok(demand)
    }

    /// Marks the surfaces whose retained image the restore left without a
    /// store: everywhere for an image no store holds, on the sampling output
    /// for one another store holds. Policy presentations that sample them
    /// are withdrawn with them.
    pub(in crate::production_visual_runtime) fn mark_unrestored_sources(
        &mut self,
        restore: &crate::LiveProductionRendererImageRestore,
    ) {
        let mut marked = Vec::new();
        for (surface, displayed) in &self.displayed_surfaces {
            let image = displayed.layer.image_id;
            let reason = crate::LiveSourceUnavailableReason::Pending(image);
            if restore.pending.contains(&image) {
                self.source_availability.mark(*surface, reason, None);
                marked.push(*surface);
                continue;
            }
            let outputs = restore
                .unavailable
                .iter()
                .filter(|(unavailable, _)| *unavailable == image)
                .map(|(_, output)| *output)
                .collect::<BTreeSet<_>>();
            if !outputs.is_empty() {
                self.source_availability
                    .mark(*surface, reason, Some(outputs));
                marked.push(*surface);
            }
        }
        if !marked.is_empty() {
            self.revoke_policy_presentation_for_removed(&marked);
        }
    }

    /// Suspension left one empty projection per retired output. Projection
    /// i belongs to output i, so a replacement that lost or gained an output
    /// gets one per resumed output, at a new epoch, as an applied topology
    /// does; otherwise a returning output would index past the end.
    pub(in crate::production_visual_runtime) fn resume_input_projections(
        &mut self,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let resumed_ids = (0..self.outputs.output_count())
            .filter_map(|index| self.outputs.output_id(index))
            .collect::<Vec<_>>();
        if !self
            .input_projections
            .iter()
            .map(|projection| projection.output)
            .eq(resumed_ids.iter().copied())
        {
            let input_epoch = self
                .input_projections
                .iter()
                .map(|projection| projection.epoch)
                .max()
                .unwrap_or(0)
                .checked_add(1)
                .ok_or("presented input projection epoch exhausted")?;
            self.input_projections = resumed_ids
                .into_iter()
                .map(|output| LivePresentedInputProjection {
                    policy_publication: None,
                    frame_completed: false,
                    policy_visible: false,
                    presented_keyboard: Default::default(),
                    output,
                    epoch: input_epoch,
                    layers: Vec::new(),
                    chrome_targets: Vec::new(),
                    chrome_occlusion: None,
                    descriptor_targets: Vec::new(),
                    descriptor_occlusion: None,
                    descriptor_projection: None,
                    tab_occlusions: Vec::new(),
                    content: Vec::new(),
                })
                .collect();
        }
        Ok(())
    }
}
