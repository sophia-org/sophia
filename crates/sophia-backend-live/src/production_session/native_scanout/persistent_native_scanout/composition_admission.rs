//! Complete-batch planning shared by native intake and simulated completion tests.
//! This owns lowered frames, but neither opens a device nor invents a completion.
use super::composition_queue::{
    LiveProductionQueuedMirrorGeneration, LiveProductionQueuedMirrorHeadFrame,
};
use super::renderer_images::LiveProductionHeadCompositionContent;
use super::*;

pub(crate) type NativeHeadCompositionBatch =
    Vec<(OutputId, Vec<LiveProductionHeadCompositionFrame>)>;

/// A read-only capture of current native head/queue facts. The native owner
/// supplies it before moving any offered frame into persistent custody.
pub(crate) struct NativeCompositionOutput {
    pub targets: Vec<(usize, sophia_engine::HeadRenderTarget)>,
    pub ready: bool,
    pub protected: bool,
    pub available: bool,
    pub newest: [Option<LiveProductionScanoutContent>; 4],
    pub settled_mirror_checksum: Option<u64>,
}

pub(crate) fn prepare_native_composition_batch(
    batches: NativeHeadCompositionBatch,
    required: &BTreeSet<OutputId>,
    outputs: &BTreeMap<OutputId, NativeCompositionOutput>,
    owner: crate::NativeFrameOwner,
    next_frame: &mut u64,
    content: LiveProductionHeadCompositionContent,
) -> Result<Vec<LiveProductionQueuedMirrorGeneration>, (&'static str, NativeHeadCompositionBatch)> {
    let validate = || {
        let mut selected = BTreeMap::new();
        for (output, frames) in &batches {
            if selected.contains_key(output) {
                return Err("composition batch repeats an output");
            }
            let state = outputs
                .get(output)
                .ok_or("composition batch targets an unknown output")?;
            if !state.available {
                return Err("composition output is unavailable");
            }
            if state.protected
                && (required.contains(output)
                    || !matches!(content, LiveProductionHeadCompositionContent::Retained))
            {
                return Err("composition output already owns a distinct retirement");
            }
            if required.contains(output) && !state.ready {
                return Err("required composition output is not ready");
            }
            let targets = state
                .targets
                .iter()
                .map(|(_, target)| *target)
                .collect::<Vec<_>>();
            let checksum = validate_live_head_composition_frame_batch(*output, &targets, frames)?;
            let suppress = state.protected
                || matches!(content, LiveProductionHeadCompositionContent::Retained)
                    && ((targets.len() == 1
                        && reduce_live_production_retained_frame_queue(
                            live_production_retained_frame_requirement(required.contains(output)),
                            state.newest[0],
                            state.newest[1],
                            state.newest[2],
                            state.newest[3],
                            checksum,
                        ) != LiveProductionRetainedSceneQueueStatus::Queue)
                        || (!required.contains(output)
                            && state.settled_mirror_checksum == Some(checksum)));
            selected.insert(*output, (checksum, suppress));
        }
        if required.iter().any(|output| !selected.contains_key(output)) {
            return Err("required retained retirement targets an absent output");
        }
        let count = selected.values().filter(|(_, suppress)| !suppress).count() as u64;
        if *next_frame == 0 || next_frame.checked_add(count).is_none() {
            return Err("native frame identity space exhausted");
        }
        Ok(selected)
    };
    let selected = match validate() {
        Ok(selected) => selected,
        Err(reason) => return Err((reason, batches)),
    };
    let mut prepared = Vec::with_capacity(selected.len());
    for (output, frames) in batches {
        let (checksum, suppress) = selected[&output];
        if suppress {
            continue;
        }
        let frame = LiveProductionNativeFrameId::from_raw(*next_frame);
        *next_frame += 1; // whole batch checked before any owner transfer
        let mut frames = frames
            .into_iter()
            .map(|frame| (frame.head, frame))
            .collect::<BTreeMap<_, _>>();
        let heads = outputs[&output]
            .targets
            .iter()
            .map(|(index, target)| {
                let prepared = frames
                    .remove(&target.head)
                    .expect("validated head coverage");
                LiveProductionQueuedMirrorHeadFrame {
                    head_index: *index,
                    identity: owner.frame(
                        output,
                        target.head,
                        target.target_generation,
                        frame.raw(),
                    ),
                    content: match content.scanout_content(frame, checksum) {
                        LiveProductionScanoutContent::RetainedMixed {
                            frame,
                            nonzero_rgb_pixels,
                            ..
                        } => LiveProductionScanoutContent::RetainedMixed {
                            logical_content_checksum: Some(checksum),
                            frame,
                            nonzero_rgb_pixels,
                            requires_retirement: required.contains(&output),
                        },
                        other => other,
                    },
                    output_damage_snapshot: prepared.frame.output_damage_snapshot.clone(),
                    frame: prepared.frame,
                    cpu_nonzero_pixel_bytes: 0,
                }
            })
            .collect();
        prepared.push(LiveProductionQueuedMirrorGeneration {
            output,
            frame,
            logical_content_checksum: Some(checksum),
            heads,
        });
    }
    Ok(prepared)
}

impl LiveProductionNativeScanout {
    pub(super) fn prepare_and_admit_head_batch(
        &mut self,
        batches: super::composition_admission::NativeHeadCompositionBatch,
        required: &BTreeSet<OutputId>,
        content: LiveProductionHeadCompositionContent,
    ) -> Result<BTreeMap<OutputId, LiveProductionNativeFrameId>, Box<dyn std::error::Error>> {
        let states = self
            .logical_outputs
            .iter()
            .map(|output| {
                let indices = self.head_indices(output.id);
                let targets = self
                    .head_render_targets(output.id)
                    .into_iter()
                    .map(|target| {
                        let index = *indices
                            .iter()
                            .find(|index| self.heads[**index].head == target.head)
                            .expect("native head target");
                        (index, target)
                    })
                    .collect();
                let newest = indices.first().map_or([None; 4], |index| {
                    let head = &self.heads[*index];
                    [
                        head.pending_content,
                        head.rendering_content,
                        head.submitted_content,
                        head.presented_content,
                    ]
                });
                (
                    output.id,
                    super::composition_admission::NativeCompositionOutput {
                        targets,
                        ready: self.frame_queue_ready(output.id),
                        protected: self.output_retirement_protected(output.id),
                        available: !self.mirror_generation_failed(output.id),
                        newest,
                        settled_mirror_checksum: super::settled_mirror::settled_mirror_checksum(
                            self.native_frame_owner,
                            output.id,
                            indices.len(),
                            self.output_lifecycles.get(&output.id),
                            indices.iter().map(|index| {
                                let head = &self.heads[*index];
                                super::settled_mirror::SettledMirrorHead {
                                    head: head.head,
                                    target_generation: head.target_generation,
                                    idle: !self.exporters[*index].pending_frame()
                                        && !self.deferred_mirror_generations.pending(output.id)
                                        && head.pending_content.is_none()
                                        && head.rendering_content.is_none()
                                        && head.submitted_content.is_none()
                                        && head.prepared_scanout.is_none()
                                        && head.scanout_custody.submitted().is_none()
                                        && !head.scanout_custody.cleanup_pending(),
                                    presented: head.presented_content,
                                    displayed: head
                                        .scanout_custody
                                        .displayed()
                                        .and_then(|value| value.correlation())
                                        .and_then(|value| value.native),
                                }
                            }),
                        ),
                    },
                )
            })
            .collect();
        let generations = super::composition_admission::prepare_native_composition_batch(
            batches,
            required,
            &states,
            self.native_frame_owner,
            &mut self.next_frame_id,
            content,
        )
        .map_err(|(reason, _unaccepted)| -> Box<dyn std::error::Error> { reason.into() })?;
        self.admit_head_composition_batch(generations)
    }

    fn admit_head_composition_batch(
        &mut self,
        generations: Vec<LiveProductionQueuedMirrorGeneration>,
    ) -> Result<BTreeMap<OutputId, LiveProductionNativeFrameId>, Box<dyn std::error::Error>> {
        let configured = self.output_lifecycles.keys().copied().collect();
        let admitted = self
            .deferred_mirror_generations
            .admit_batch(generations, &configured)
            .map_err(|(reason, _unaccepted)| -> Box<dyn std::error::Error> { reason.into() })?;
        // Queue evidence follows the complete owned transfer, never preparation.
        for output in admitted.keys() {
            let generation = self
                .deferred_mirror_generations
                .get(*output)
                .expect("admitted owner");
            for queued in &generation.heads {
                let head = &self.heads[queued.head_index];
                let damage = queued
                    .frame
                    .output_damage_snapshot
                    .as_ref()
                    .expect("validated head damage");
                tracing::info!(
                    "sophia_live_head_composition_queue schema=1 status=queued output={} head={} frame={} scene_generation={} target_generation={} mapping={} width={} height={} logical_content_checksum={} source=head_plan",
                    output.raw(),
                    head.head.raw(),
                    generation.frame.raw(),
                    queued.frame.trace.map_or(0, |trace| trace.scene_generation),
                    queued.identity.target_generation(),
                    head.mapping.reduced_name(),
                    damage.output.size.width,
                    damage.output.size.height,
                    generation.logical_checksum().unwrap_or_default(),
                );
            }
        }
        Ok(admitted)
    }
}
