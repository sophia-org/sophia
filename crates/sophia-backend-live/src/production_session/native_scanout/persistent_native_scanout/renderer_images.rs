use super::composition_queue::{
    LiveProductionQueuedMirrorGeneration, LiveProductionQueuedMirrorHeadFrame,
};
use super::*;

#[derive(Debug)]
pub struct LiveProductionHeadCompositionFrame {
    pub head: sophia_engine::RenderHeadId,
    pub scene_generation: u64,
    pub target_generation: u64,
    pub mapping: sophia_protocol::OutputHeadMapping,
    pub logical_content_checksum: u64,
    pub frame: crate::LiveOwnedMixedCompositionFrame,
}

/// Returns the renderer-image identities that each physical head must own
/// before its lowered frame can enter a renderer worker.
///
/// Renderer-image identities are local to one head's persistent renderer
/// store. A topology change can assign an already-retained logical scene to a
/// head that has never rendered it, so frame coverage alone is not sufficient
/// preparation.
pub fn live_topology_frame_renderer_image_requirements(
    frames: &BTreeMap<sophia_engine::RenderHeadId, LiveProductionHeadCompositionFrame>,
) -> BTreeMap<sophia_engine::RenderHeadId, Vec<sophia_renderer_live::LiveRendererImageId>> {
    frames
        .iter()
        .filter_map(|(head, frame)| {
            let image_ids = frame
                .frame
                .layers
                .iter()
                .filter_map(|layer| match layer {
                    sophia_renderer_live::LiveOwnedMixedCompositionLayer::RendererImage {
                        image_id,
                        ..
                    } => Some(*image_id),
                    _ => None,
                })
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            (!image_ids.is_empty()).then_some((*head, image_ids))
        })
        .collect()
}

/// Validates the complete passive Engine-plan batch before any exporter slot is
/// mutated. The batch is one immutable logical scene, but every member must
/// retain its own current target generation, mapping, and native damage extent.
pub fn validate_live_head_composition_frame_batch(
    output: OutputId,
    expected: &[sophia_engine::HeadRenderTarget],
    frames: &[LiveProductionHeadCompositionFrame],
) -> Result<u64, &'static str> {
    if expected.is_empty() || expected.len() != frames.len() {
        return Err("head composition does not cover the output's physical heads");
    }
    let expected_heads = expected
        .iter()
        .map(|target| target.head)
        .collect::<BTreeSet<_>>();
    let actual_heads = frames
        .iter()
        .map(|frame| frame.head)
        .collect::<BTreeSet<_>>();
    if expected_heads != actual_heads || actual_heads.len() != frames.len() {
        return Err("head composition repeats or targets an unknown physical head");
    }
    let checksum = frames
        .first()
        .map(|frame| frame.logical_content_checksum)
        .ok_or("head composition is empty")?;
    if frames
        .iter()
        .any(|frame| frame.logical_content_checksum != checksum)
    {
        return Err("head composition frames disagree on logical content");
    }
    let scene_generation = frames
        .first()
        .map(|frame| frame.scene_generation)
        .filter(|generation| *generation != 0)
        .ok_or("head composition has an invalid scene generation")?;
    if frames
        .iter()
        .any(|frame| frame.scene_generation != scene_generation)
    {
        return Err("head composition frames disagree on scene generation");
    }
    for frame in frames {
        let target = expected
            .iter()
            .find(|target| target.head == frame.head)
            .expect("head coverage checked above");
        let Some(damage) = frame.frame.output_damage_snapshot.as_ref() else {
            return Err("head composition frame has no native damage snapshot");
        };
        if target.output != output
            || damage.output
                != (sophia_engine::HeadlessOutput {
                    id: output,
                    size: target.native_size,
                    scale: target.scale,
                })
        {
            return Err("head composition damage does not match its native target");
        }
        if frame.target_generation != target.target_generation {
            return Err("head composition targets a stale native generation");
        }
        if frame.mapping != target.mapping {
            return Err("head composition mapping does not match its native target");
        }
    }
    Ok(checksum)
}

#[derive(Clone, Copy)]
pub(crate) enum LiveProductionHeadCompositionContent {
    Scene,
    /// Replaceable scene work; a protected output defers without a new ID.
    OrdinaryScene,
    MixedPresent(TransactionId),
    Retained,
    RetainedFresh,
}

impl LiveProductionHeadCompositionContent {
    pub(crate) fn scanout_content(
        self,
        frame: LiveProductionNativeFrameId,
        logical_content_checksum: u64,
    ) -> LiveProductionScanoutContent {
        match self {
            Self::Scene | Self::OrdinaryScene => LiveProductionScanoutContent::HeadComposition {
                frame,
                logical_content_checksum,
                nonzero_rgb_pixels: 0,
            },
            Self::MixedPresent(transaction) => LiveProductionScanoutContent::MixedPresent {
                frame,
                transaction,
                nonzero_rgb_pixels: 0,
            },
            Self::Retained | Self::RetainedFresh => LiveProductionScanoutContent::RetainedMixed {
                logical_content_checksum: Some(logical_content_checksum),
                requires_retirement: false,
                frame,
                nonzero_rgb_pixels: 0,
            },
        }
    }
}

fn project_owned_mixed_frame(
    frame: &crate::LiveOwnedMixedCompositionFrame,
    source: sophia_protocol::Size,
    destination: sophia_engine::HeadlessOutput,
    fit: sophia_protocol::OutputHeadMapping,
) -> Result<crate::LiveOwnedMixedCompositionFrame, Box<dyn std::error::Error>> {
    if source.width <= 0 || source.height <= 0 {
        return Err("mirror mixed-frame source size is invalid".into());
    }
    let target = crate::project_mirror_rect(source, destination.size, fit);
    if target.width <= 0 || target.height <= 0 {
        return Err("mirror mixed-frame projection is empty".into());
    }
    let mut projected = crate::try_clone_mixed_frame(frame)?;
    for layer in &mut projected.layers {
        match layer {
            sophia_renderer_live::LiveOwnedMixedCompositionLayer::Cpu { placement, .. }
            | sophia_renderer_live::LiveOwnedMixedCompositionLayer::DmaBuf { placement, .. }
            | sophia_renderer_live::LiveOwnedMixedCompositionLayer::RendererImage {
                placement,
                ..
            } => {
                placement.target =
                    crate::project_mirror_child_rect(placement.target, source, target);
                placement.clip = placement
                    .clip
                    .map(|clip| crate::project_mirror_child_rect(clip, source, target));
            }
            sophia_renderer_live::LiveOwnedMixedCompositionLayer::Solid { geometry, .. } => {
                *geometry = crate::project_mirror_child_rect(*geometry, source, target);
            }
        }
    }
    projected.output_damage_snapshot = frame
        .output_damage_snapshot
        .as_ref()
        .map(|snapshot| project_mirror_output_damage_snapshot(snapshot, source, destination, fit))
        .transpose()?;
    Ok(projected)
}

impl LiveProductionNativeScanout {
    fn mirror_mixed_transaction_frame(
        &self,
        output: OutputId,
        transaction: TransactionId,
    ) -> Option<LiveProductionNativeFrameId> {
        self.head_indices(output)
            .into_iter()
            .find_map(|head_index| {
                let head = &self.heads[head_index];
                [
                    head.pending_content,
                    head.rendering_content,
                    head.submitted_content,
                ]
                .into_iter()
                .flatten()
                .find_map(|content| match content {
                    LiveProductionScanoutContent::MixedPresent {
                        frame,
                        transaction: owned,
                        ..
                    } if owned == transaction => Some(frame),
                    _ => None,
                })
            })
    }

    fn mirror_generation_content(
        &self,
        output: OutputId,
        frame: LiveProductionNativeFrameId,
    ) -> Option<LiveProductionScanoutContent> {
        self.head_indices(output)
            .into_iter()
            .find_map(|head_index| {
                let head = &self.heads[head_index];
                [
                    head.pending_content,
                    head.rendering_content,
                    head.submitted_content,
                    head.presented_content,
                ]
                .into_iter()
                .flatten()
                .find(|content| content.frame() == frame)
            })
    }

    fn install_mirror_generation(
        &mut self,
        generation: LiveProductionQueuedMirrorGeneration,
        status: &'static str,
    ) -> Result<(), (&'static str, LiveProductionQueuedMirrorGeneration)> {
        let source = generation.source();
        let checksum = generation.logical_checksum();
        let output = generation.output;
        let frame = generation.frame;
        composition_installation::install_composition_generation(self, generation)?;
        tracing::info!(
            "sophia_live_mirror_generation schema=2 status={} output={} frame={} source={} logical_content_checksum={}",
            status,
            output.raw(),
            frame.raw(),
            source,
            checksum.map_or_else(|| "none".to_owned(), |checksum| checksum.to_string()),
        );
        Ok(())
    }

    fn queue_mirror_generation(
        &mut self,
        generation: LiveProductionQueuedMirrorGeneration,
    ) -> Result<(), &'static str> {
        let output = generation.output;
        let frame = generation.frame;
        let Some(lifecycle) = self.output_lifecycles.get(&output) else {
            self.deferred_mirror_generations
                .retain_refused(generation)
                .map_err(|_unaccepted| "composition queue retains an earlier retirement")?;
            return Err("mirror generation targets an unregistered output");
        };
        let previous = lifecycle.active_frame();
        let primary_owned = lifecycle
            .logically_submitted_frame()
            .or_else(|| lifecycle.displayed_frame(lifecycle.primary_head()));
        let active_content =
            previous.and_then(|frame| self.mirror_generation_content(output, frame));
        let generation = match self.deferred_mirror_generations.offer(
            generation,
            previous,
            primary_owned,
            active_content,
        ) {
            composition_queue::DeferredCompositionOffer::Install(generation) => generation,
            composition_queue::DeferredCompositionOffer::Refused(_unaccepted) => {
                return Err("composition queue retains an earlier retirement");
            }
            composition_queue::DeferredCompositionOffer::Deferred { replaced } => {
                tracing::trace!(
                    "sophia_live_mirror_pacing schema=1 status=deferred output={} frame={} blocked_by={} replaced={}",
                    output.raw(),
                    frame.raw(),
                    previous
                        .expect("deferred generation has an active predecessor")
                        .raw(),
                    replaced.map_or_else(|| "none".to_owned(), |frame| frame.raw().to_string()),
                );
                return Ok(());
            }
        };
        if let Err((reason, generation)) = self.install_mirror_generation(
            generation,
            if previous.is_some() {
                "coalesced"
            } else {
                "installed"
            },
        ) {
            self.deferred_mirror_generations
                .retain_refused(generation)
                .map_err(|_unaccepted| "composition queue retains an earlier retirement")?;
            return Err(reason);
        }
        if let Some(previous) = previous {
            tracing::trace!(
                "sophia_live_mirror_pacing schema=1 status=newest_ready output={} frame={} previous={}",
                output.raw(),
                frame.raw(),
                previous.raw(),
            );
        }
        Ok(())
    }

    pub fn activate_deferred_mirror_generation(
        &mut self,
        output: OutputId,
    ) -> Result<bool, &'static str> {
        if !self.deferred_mirror_generations.pending(output) {
            return Ok(false);
        }
        let Some(lifecycle) = self.output_lifecycles.get(&output) else {
            return Err("queued composition targets an unregistered output");
        };
        if self.installed_retirement_protected(output) {
            return Ok(false);
        }
        let active = lifecycle.active_frame();
        let primary_owned = lifecycle
            .logically_submitted_frame()
            .or_else(|| lifecycle.displayed_frame(lifecycle.primary_head()));
        let active_content = active.and_then(|frame| self.mirror_generation_content(output, frame));
        let Some(generation) = self.deferred_mirror_generations.take_ready(
            output,
            active,
            primary_owned,
            active_content,
        ) else {
            return Ok(false);
        };
        self.queue_mirror_generation(generation)?;
        Ok(true)
    }

    /// Queues already-lowered, native-size frames without projecting any head
    /// from another head's pixels. Every physical head must appear exactly
    /// once and must carry the same logical scene checksum.
    pub fn queue_head_composition_frames(
        &mut self,
        output: OutputId,
        frames: Vec<LiveProductionHeadCompositionFrame>,
    ) -> Result<LiveProductionNativeFrameId, Box<dyn std::error::Error>> {
        self.queue_head_composition_frames_with_content(
            output,
            frames,
            LiveProductionHeadCompositionContent::Scene,
        )
    }

    /// Installs the first semantic frame for every head without reserving an
    /// ordinary page-flip generation.
    ///
    /// Startup presents these frames through one blocking card-scoped modeset,
    /// so there is no later callback to complete `LiveProductionMirrorGroupLifecycle`.
    /// The caller must prepare every queued head before submitting any KMS
    /// mutation and then mark the initial presentation synchronously.
    pub(super) fn queue_initial_head_composition_frames(
        &mut self,
        output: OutputId,
        frames: Vec<LiveProductionHeadCompositionFrame>,
    ) -> Result<LiveProductionNativeFrameId, Box<dyn std::error::Error>> {
        let (indices, checksum) = self.validate_head_composition_frames(output, &frames)?;
        if indices.is_empty() {
            return Err("semantic startup requires at least one head".into());
        }
        if self
            .output_lifecycles
            .get(&output)
            .and_then(LiveProductionMirrorGroupLifecycle::active_frame)
            .is_some()
            || indices.iter().any(|index| {
                self.heads[*index].pending_content.is_some()
                    || self.heads[*index].scanout_custody.displayed().is_some()
                    || self.exporters[*index].pending_frame()
            })
        {
            return Err("semantic multi-head startup found pre-existing head work".into());
        }
        let frame_id = self.allocate_frame_id();
        let mut by_head = frames
            .into_iter()
            .map(|frame| (frame.head, frame))
            .collect::<BTreeMap<_, _>>();
        for head_index in indices {
            let prepared = by_head
                .remove(&self.heads[head_index].head)
                .expect("initial head coverage checked above");
            let damage = prepared
                .frame
                .output_damage_snapshot
                .as_ref()
                .expect("initial head damage checked above");
            tracing::info!(
                "sophia_live_head_composition_queue schema=1 status=queued output={} head={} frame={} scene_generation={} target_generation={} mapping={} width={} height={} logical_content_checksum={} source=head_plan",
                output.raw(),
                prepared.head.raw(),
                frame_id.raw(),
                prepared.scene_generation,
                prepared.target_generation,
                prepared.mapping.reduced_name(),
                damage.output.size.width,
                damage.output.size.height,
                checksum,
            );
            let identity = self.native_frame_identity(head_index, output, frame_id);
            let (head, exporter) = self.head_and_exporter(head_index, output);
            head.last_checksum = checksum;
            head.pending_content = Some(LiveProductionScanoutContent::HeadComposition {
                frame: frame_id,
                logical_content_checksum: checksum,
                nonzero_rgb_pixels: 0,
            });
            head.queue_output_damage_snapshot(prepared.frame.output_damage_snapshot.clone());
            exporter.set_pending_identified_mixed_frame(prepared.frame, Some(identity));
        }
        Ok(frame_id)
    }

    pub fn queue_present_head_composition_frames(
        &mut self,
        output: OutputId,
        transaction: TransactionId,
        frames: Vec<LiveProductionHeadCompositionFrame>,
    ) -> Result<LiveProductionNativeFrameId, Box<dyn std::error::Error>> {
        self.queue_head_composition_frames_with_content(
            output,
            frames,
            LiveProductionHeadCompositionContent::MixedPresent(transaction),
        )
    }

    /// Admits every logical-output cohort of one Present only after the whole
    /// cross-output batch has passed head coverage, damage, and readiness
    /// validation. Once queueing starts, any unexpected failure is a fatal
    /// invariant rather than permission to publish a partial transaction.
    pub fn queue_present_output_head_composition_frames(
        &mut self,
        transaction: TransactionId,
        batches: Vec<(OutputId, Vec<LiveProductionHeadCompositionFrame>)>,
    ) -> Result<BTreeMap<OutputId, LiveProductionNativeFrameId>, Box<dyn std::error::Error>> {
        if batches.is_empty() {
            return Err("Present has no applicable logical output".into());
        }
        let required = batches.iter().map(|(output, _)| *output).collect();
        self.prepare_and_admit_head_batch(
            batches,
            &required,
            LiveProductionHeadCompositionContent::MixedPresent(transaction),
        )
    }

    pub fn queue_retained_head_composition_frames(
        &mut self,
        output: OutputId,
        frames: Vec<LiveProductionHeadCompositionFrame>,
    ) -> Result<LiveProductionNativeFrameId, Box<dyn std::error::Error>> {
        self.queue_head_composition_frames_with_content(
            output,
            frames,
            LiveProductionHeadCompositionContent::Retained,
        )
    }

    pub fn queue_retained_output_head_composition_frames(
        &mut self,
        batches: Vec<(OutputId, Vec<LiveProductionHeadCompositionFrame>)>,
    ) -> Result<BTreeMap<OutputId, LiveProductionNativeFrameId>, Box<dyn std::error::Error>> {
        self.queue_retained_output_head_composition_frames_requiring_retirement(
            batches,
            &BTreeSet::new(),
        )
    }

    /// Queues an ordinary retained projection while preserving the distinct
    /// retirement owed by an accepted shell content candidate.
    ///
    /// Other outputs keep latest-scene suppression. A candidate output must
    /// cross a new native presentation even when its raster is byte-identical
    /// to the one already displayed.
    pub fn queue_retained_output_head_composition_frames_requiring_retirement(
        &mut self,
        batches: Vec<(OutputId, Vec<LiveProductionHeadCompositionFrame>)>,
        required_outputs: &BTreeSet<OutputId>,
    ) -> Result<BTreeMap<OutputId, LiveProductionNativeFrameId>, Box<dyn std::error::Error>> {
        let batch_outputs = batches
            .iter()
            .map(|(output, _)| *output)
            .collect::<BTreeSet<_>>();
        if !required_outputs.is_subset(&batch_outputs) {
            return Err("required retained retirement targets an absent output".into());
        }
        self.queue_retained_output_head_composition_frames_with_requirements(
            batches,
            required_outputs,
        )
    }

    /// Whether every output with a protocol retirement debt can accept its
    /// replacement without superseding another native frame.
    pub fn retained_retirements_ready(&self, required_outputs: &BTreeSet<OutputId>) -> bool {
        required_outputs.iter().all(|output| {
            self.frame_queue_ready(*output) && !self.output_retirement_protected(*output)
        })
    }

    pub(crate) fn output_retirement_protected(&self, output: OutputId) -> bool {
        self.deferred_mirror_generations.protected(output)
            || self.installed_retirement_protected(output)
    }

    fn installed_retirement_protected(&self, output: OutputId) -> bool {
        self.head_indices(output).iter().any(|index| {
            let head = &self.heads[*index];
            [
                head.pending_content,
                head.rendering_content,
                head.submitted_content,
            ]
            .into_iter()
            .flatten()
            .any(LiveProductionScanoutContent::requires_retirement)
        })
    }

    pub(crate) fn retained_repaint_deferred(&self) -> bool {
        self.logical_outputs
            .iter()
            .any(|output| self.output_retirement_protected(output.id))
    }

    /// Queues one immutable software-Present cohort on every applicable
    /// logical output.
    ///
    /// Unlike an ordinary retained projection, this must never reuse an
    /// identical pending or displayed scene. The new frame is the physical
    /// clock owner for Present feedback, so suppressing it would erase the
    /// retirement the client is waiting for.
    pub fn queue_software_present_output_head_composition_frames(
        &mut self,
        batches: Vec<(OutputId, Vec<LiveProductionHeadCompositionFrame>)>,
    ) -> Result<BTreeMap<OutputId, LiveProductionNativeFrameId>, Box<dyn std::error::Error>> {
        self.queue_retained_output_head_composition_frames_with_requirement(
            batches,
            LiveProductionRetainedFrameQueueRequirement::FreshRetirement,
        )
    }

    fn queue_retained_output_head_composition_frames_with_requirement(
        &mut self,
        batches: Vec<(OutputId, Vec<LiveProductionHeadCompositionFrame>)>,
        requirement: LiveProductionRetainedFrameQueueRequirement,
    ) -> Result<BTreeMap<OutputId, LiveProductionNativeFrameId>, Box<dyn std::error::Error>> {
        let required_outputs = match requirement {
            LiveProductionRetainedFrameQueueRequirement::LatestScene => BTreeSet::new(),
            LiveProductionRetainedFrameQueueRequirement::FreshRetirement => {
                batches.iter().map(|(output, _)| *output).collect()
            }
        };
        self.queue_retained_output_head_composition_frames_with_requirements(
            batches,
            &required_outputs,
        )
    }

    fn queue_retained_output_head_composition_frames_with_requirements(
        &mut self,
        batches: Vec<(OutputId, Vec<LiveProductionHeadCompositionFrame>)>,
        required_outputs: &BTreeSet<OutputId>,
    ) -> Result<BTreeMap<OutputId, LiveProductionNativeFrameId>, Box<dyn std::error::Error>> {
        self.prepare_and_admit_head_batch(
            batches,
            required_outputs,
            LiveProductionHeadCompositionContent::Retained,
        )
    }

    fn queue_head_composition_frames_with_content(
        &mut self,
        output: OutputId,
        frames: Vec<LiveProductionHeadCompositionFrame>,
        content: LiveProductionHeadCompositionContent,
    ) -> Result<LiveProductionNativeFrameId, Box<dyn std::error::Error>> {
        // Explicit single-output callers historically enqueue a fresh frame,
        // including latest-scene replacement while a worker is busy.
        let content = match content {
            LiveProductionHeadCompositionContent::Retained => {
                LiveProductionHeadCompositionContent::RetainedFresh
            }
            other => other,
        };
        let admitted =
            self.prepare_and_admit_head_batch(vec![(output, frames)], &BTreeSet::new(), content)?;
        Ok(admitted[&output])
    }

    fn validate_head_composition_frames(
        &self,
        output: OutputId,
        frames: &[LiveProductionHeadCompositionFrame],
    ) -> Result<(Vec<usize>, u64), Box<dyn std::error::Error>> {
        let indices = self.head_indices(output);
        let expected = self.head_render_targets(output);
        let checksum = validate_live_head_composition_frame_batch(output, &expected, frames)?;
        Ok((indices, checksum))
    }

    pub fn diagnose_mixed_frame(
        &mut self,
        output: OutputId,
        frame: crate::LiveOwnedMixedCompositionFrame,
    ) -> (
        crate::LiveRendererScanoutBufferExportStatus,
        crate::LiveRendererScanoutBufferExportDetail,
    ) {
        use crate::LiveRenderedScanoutBufferExporter as _;

        let index = self
            .primary_head_index(output)
            .expect("native mixed-frame diagnosis targets a registered output");
        let (head, exporter) = self.head_and_exporter(index, output);
        exporter.set_pending_mixed_frame(frame);
        let size = head.output.size;
        let export =
            exporter.export_rendered_scanout_buffer(crate::LiveGbmEglFrameTargetRecord::new(size));
        let status = export.status;
        let detail = export.detail;
        drop(export);
        (status, detail)
    }

    pub fn evict_renderer_image(
        &mut self,
        image_id: sophia_renderer_live::LiveRendererImageId,
    ) -> Result<usize, crate::LiveRendererScanoutBufferExportDetail> {
        let mut evicted = 0usize;
        for exporter in self.exporters.iter_mut() {
            evicted = evicted.saturating_add(usize::from(exporter.evict_renderer_image(image_id)?));
        }
        Ok(evicted)
    }

    pub fn promote_renderer_image(
        &mut self,
        image_id: sophia_renderer_live::LiveRendererImageId,
    ) -> Result<usize, crate::LiveRendererScanoutBufferExportDetail> {
        let mut promoted = 0usize;
        for exporter in self.exporters.iter_mut() {
            promoted =
                promoted.saturating_add(usize::from(exporter.promote_renderer_image(image_id)?));
        }
        Ok(promoted)
    }

    pub fn rollback_renderer_image(
        &mut self,
        image_id: sophia_renderer_live::LiveRendererImageId,
    ) -> Result<usize, crate::LiveRendererScanoutBufferExportDetail> {
        let mut rolled_back = 0usize;
        for exporter in self.exporters.iter_mut() {
            rolled_back = rolled_back
                .saturating_add(usize::from(exporter.rollback_renderer_image(image_id)?));
        }
        Ok(rolled_back)
    }

    /// Every head's exporter has an initialized renderer-image owner.
    ///
    /// Counted over exporters, which are one per physical head and index-
    /// parallel with them -- including inside a mirror group, whose heads each
    /// own one. The store those owners reach is device-wide once outputs share
    /// a worker; what is per head here is the owner, not the images.
    pub fn renderer_image_owners_initialized(&self) -> bool {
        !self.exporters.is_empty()
            && self
                .exporters
                .iter()
                .all(crate::NativeGbmRenderedScanoutBufferDiscoveryExporter::renderer_image_owner_initialized)
    }

    pub fn clear_renderer_images(
        &mut self,
    ) -> Result<usize, crate::LiveRendererScanoutBufferExportDetail> {
        let mut evicted = 0usize;
        for exporter in self.exporters.iter_mut() {
            evicted = evicted.saturating_add(exporter.clear_renderer_images()?);
        }
        Ok(evicted)
    }

    pub fn export_attempts(&self) -> usize {
        self.exporters
            .iter()
            .map(crate::NativeGbmRenderedScanoutBufferDiscoveryExporter::cpu_frame_export_attempts)
            .chain(self.exporters.iter().map(
                crate::NativeGbmRenderedScanoutBufferDiscoveryExporter::mixed_frame_export_attempts,
            ))
            .sum()
    }

    pub fn mixed_exports(&self) -> usize {
        self.exporters
            .iter()
            .map(crate::NativeGbmRenderedScanoutBufferDiscoveryExporter::mixed_frame_exports)
            .sum()
    }

    pub fn persistent_render_metrics(&self) -> LivePersistentRenderMetrics {
        // Counted once for the session rather than folded per exporter: with
        // outputs sharing a thread, summing what each head can reach would
        // report the head count and hide the very collapse being measured.
        let renderer_workers = self.renderer_worker_count();
        let folded = self.exporters.iter().fold(
            LivePersistentRenderMetrics::default(),
            |mut metrics, exporter| {
                let stats = exporter.persistent_render_stats();
                metrics.target_creations = metrics
                    .target_creations
                    .saturating_add(stats.target_creations);
                metrics.target_recreations = metrics
                    .target_recreations
                    .saturating_add(stats.target_recreations);
                metrics.pipeline_creations = metrics
                    .pipeline_creations
                    .saturating_add(stats.gl_pipeline_creations);
                metrics.frame_surface_creations = metrics
                    .frame_surface_creations
                    .saturating_add(stats.frame_surface_creations);
                metrics.cpu_target_creations = metrics
                    .cpu_target_creations
                    .saturating_add(stats.cpu_target_creations);
                metrics.dmabuf_target_creations = metrics
                    .dmabuf_target_creations
                    .saturating_add(stats.dmabuf_target_creations);
                metrics.composition_target_creations = metrics
                    .composition_target_creations
                    .saturating_add(stats.composition_target_creations);
                metrics.composition_target_reuses = metrics
                    .composition_target_reuses
                    .saturating_add(stats.composition_target_reuses);
                metrics.generation_replacements = metrics
                    .generation_replacements
                    .saturating_add(stats.generation_replacements);
                metrics.recovery_replacements = metrics
                    .recovery_replacements
                    .saturating_add(stats.recovery_replacements);
                metrics.uploads = metrics.uploads.saturating_add(stats.frame_uploads);
                metrics.snapshot_captures = metrics
                    .snapshot_captures
                    .saturating_add(stats.snapshot_captures);
                metrics.snapshot_promotions = metrics
                    .snapshot_promotions
                    .saturating_add(stats.snapshot_promotions);
                metrics.snapshot_rollbacks = metrics
                    .snapshot_rollbacks
                    .saturating_add(stats.snapshot_rollbacks);
                metrics.snapshot_evictions = metrics
                    .snapshot_evictions
                    .saturating_add(stats.snapshot_evictions);
                metrics.snapshot_live_entries = metrics
                    .snapshot_live_entries
                    .saturating_add(stats.snapshot_live_entries);
                metrics.snapshot_live_bytes = metrics
                    .snapshot_live_bytes
                    .saturating_add(stats.snapshot_live_bytes);
                metrics.import_cache_imports = metrics
                    .import_cache_imports
                    .saturating_add(stats.import_cache.imports);
                metrics.import_cache_hits = metrics
                    .import_cache_hits
                    .saturating_add(stats.import_cache.hits);
                metrics.import_cache_evictions = metrics
                    .import_cache_evictions
                    .saturating_add(stats.import_cache.evictions);
                metrics.import_cache_live_entries = metrics
                    .import_cache_live_entries
                    .saturating_add(stats.import_cache.live_entries);
                metrics.import_cache_descriptor_mismatches = metrics
                    .import_cache_descriptor_mismatches
                    .saturating_add(stats.import_cache.descriptor_mismatches);
                metrics.import_cache_capacity_rejections = metrics
                    .import_cache_capacity_rejections
                    .saturating_add(stats.import_cache.capacity_rejections);
                metrics.exact_nearest_draws = metrics
                    .exact_nearest_draws
                    .saturating_add(stats.exact_nearest_draws);
                metrics.sharp_downscale_draws = metrics
                    .sharp_downscale_draws
                    .saturating_add(stats.sharp_downscale_draws);
                metrics.sharp_upscale_draws = metrics
                    .sharp_upscale_draws
                    .saturating_add(stats.sharp_upscale_draws);
                metrics.linear_fallback_draws = metrics
                    .linear_fallback_draws
                    .saturating_add(stats.linear_fallback_draws);
                if let Some(worker) = exporter.worker_metrics() {
                    metrics.worker_requests =
                        metrics.worker_requests.saturating_add(worker.requests);
                    metrics.worker_completions = metrics
                        .worker_completions
                        .saturating_add(worker.completions);
                    metrics.worker_failures =
                        metrics.worker_failures.saturating_add(worker.failures);
                    metrics.worker_soft_stalls = metrics
                        .worker_soft_stalls
                        .saturating_add(worker.soft_stalls);
                    metrics.worker_hard_stalls = metrics
                        .worker_hard_stalls
                        .saturating_add(worker.hard_stalls);
                    metrics.worker_release_enqueue_failures = metrics
                        .worker_release_enqueue_failures
                        .saturating_add(worker.release_enqueue_failures);
                    metrics.worker_result_misroutes = metrics
                        .worker_result_misroutes
                        .saturating_add(worker.result_misroutes);
                    metrics.frame_slot_acquisitions = metrics
                        .frame_slot_acquisitions
                        .saturating_add(worker.frame_slots.acquisitions);
                    metrics.frame_slot_reuses = metrics
                        .frame_slot_reuses
                        .saturating_add(worker.frame_slots.reuses);
                    metrics.frame_slot_deferrals = metrics
                        .frame_slot_deferrals
                        .saturating_add(worker.frame_slots.deferrals);
                    metrics.frame_slot_stale_releases = metrics
                        .frame_slot_stale_releases
                        .saturating_add(worker.frame_slots.stale_releases);
                    metrics.frame_slots_leased = metrics
                        .frame_slots_leased
                        .saturating_add(worker.frame_slots.leased);
                    metrics.frame_slots_high_watermark = metrics
                        .frame_slots_high_watermark
                        .saturating_add(worker.frame_slots.high_watermark);
                    metrics.frame_slot_partial_repaints = metrics
                        .frame_slot_partial_repaints
                        .saturating_add(worker.frame_slots.partial_repaints);
                    metrics.frame_slot_full_repaints = metrics
                        .frame_slot_full_repaints
                        .saturating_add(worker.frame_slots.full_repaints);
                    metrics.frame_slot_history_invalidations = metrics
                        .frame_slot_history_invalidations
                        .saturating_add(worker.frame_slots.history_invalidations);
                    metrics.frame_slot_history_records = metrics
                        .frame_slot_history_records
                        .saturating_add(worker.frame_slots.history_records);
                    metrics.max_worker_request =
                        metrics.max_worker_request.max(worker.max_request_age);
                }
                metrics.max_target_create = metrics.max_target_create.max(stats.max_target_create);
                metrics.max_frame_surface_create = metrics
                    .max_frame_surface_create
                    .max(stats.max_frame_surface_create);
                metrics.max_render = metrics.max_render.max(stats.max_render);
                metrics.max_upload = metrics.max_upload.max(stats.max_upload);
                metrics
            },
        );
        LivePersistentRenderMetrics {
            renderer_workers,
            ..folded
        }
    }
}

include!("output_frame_queue.rs");
