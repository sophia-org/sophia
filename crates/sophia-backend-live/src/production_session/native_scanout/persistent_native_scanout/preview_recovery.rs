//! Only unsubmitted preparation failures can withdraw a publication. Physical
//! KMS owners and unrelated renderer/device failures remain on their old path.
use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LivePreviewFrameFailure {
    pub output: OutputId,
    pub frame: LiveProductionNativeFrameId,
    pub owner_epoch: u64,
    pub publication: u64,
    pub source: sophia_protocol::SurfaceId,
    pub required_retirement: bool,
    pub detail: crate::LiveRendererScanoutBufferExportDetail,
    recorded_at: Instant,
    recovery_started_at: Option<Instant>,
}

fn recoverable(detail: crate::LiveRendererScanoutBufferExportDetail) -> bool {
    use crate::LiveRendererScanoutBufferExportDetail as D;
    matches!(
        detail,
        D::InvalidRendererImageId | D::RendererImageStoreFull | D::RendererImageTransferBusy
    )
}

impl LivePreviewFrameFailure {
    #[cfg(test)]
    pub(crate) fn test_failure(
        output: OutputId,
        frame: LiveProductionNativeFrameId,
        owner_epoch: u64,
        publication: u64,
        source: sophia_protocol::SurfaceId,
    ) -> Self {
        Self {
            output,
            frame,
            owner_epoch,
            publication,
            source,
            required_retirement: true,
            detail: crate::LiveRendererScanoutBufferExportDetail::InvalidRendererImageId,
            recorded_at: Instant::now(),
            recovery_started_at: None,
        }
    }

    fn check_deadline(
        self,
        now: Instant,
    ) -> Result<(), crate::LiveRendererScanoutBufferExportDetail> {
        if self.recovery_started_at.is_some_and(|started| {
            now.saturating_duration_since(started) >= crate::LIVE_RENDERER_WORKER_HARD_STALL
        }) {
            Err(crate::LiveRendererScanoutBufferExportDetail::WorkerStalled)
        } else {
            Ok(())
        }
    }
}

impl LiveProductionNativeScanout {
    pub(super) fn prune_preview_frame_sources(&mut self) {
        let mut active = BTreeSet::new();
        for (index, head) in self.heads.iter().enumerate() {
            for content in [
                head.pending_content,
                head.rendering_content,
                head.submitted_content,
            ]
            .into_iter()
            .flatten()
            {
                active.insert(self.native_frame_identity(index, head.output.id, content.frame()));
            }
        }
        let queued = self
            .logical_outputs
            .iter()
            .filter_map(|output| self.deferred_mirror_generations.get(output.id))
            .flat_map(|generation| generation.heads.iter().map(|head| head.identity))
            .collect::<BTreeSet<_>>();
        self.preview_images
            .frame_sources
            .retain(|identity, _| active.contains(identity) || queued.contains(identity));
    }

    pub(super) fn record_preview_failure(
        &mut self,
        failure: LivePreviewFrameFailure,
    ) -> Result<(), Box<dyn std::error::Error>> {
        record_failure(&mut self.preview_failures, failure).map_err(Into::into)
    }

    /// Authoritative for all tick callers, including the Present driver. The
    /// deadline remains active even after the mirror lifecycle is withdrawn.
    pub(super) fn preview_recovery_blocks(
        &self,
        output: OutputId,
    ) -> Result<bool, crate::LiveRendererScanoutBufferExportDetail> {
        recovery_blocks(&self.preview_failures, output, Instant::now())
    }

    pub(crate) fn queue_preview_present_replacement(
        &mut self,
        failure: LivePreviewFrameFailure,
        transaction: Option<TransactionId>,
        batches: composition_admission::NativeHeadCompositionBatch,
    ) -> Result<BTreeMap<OutputId, LiveProductionNativeFrameId>, Box<dyn std::error::Error>> {
        if self.preview_failures.get(&failure.output) != Some(&failure)
            || batches.len() != 1
            || batches[0].0 != failure.output
            || self.head_indices(failure.output).iter().any(|index| {
                let head = &self.heads[*index];
                [
                    head.pending_content,
                    head.rendering_content,
                    head.submitted_content,
                ]
                .into_iter()
                .flatten()
                .any(|content| content.frame() == failure.frame)
                    || head.prepared_group_frame == Some(failure.frame)
                    || head.scanout_custody.cleanup_pending()
                    || self.exporters[*index].worker_in_flight()
            })
        {
            return Err("preview replacement does not own the withdrawn output".into());
        }
        let (content, required) = match transaction {
            Some(transaction) => (
                renderer_images::LiveProductionHeadCompositionContent::MixedPresent(transaction),
                BTreeSet::new(),
            ),
            None => (
                renderer_images::LiveProductionHeadCompositionContent::RetainedFresh,
                BTreeSet::from([failure.output]),
            ),
        };
        self.prepare_and_admit_head_batch_for_recovery(batches, &required, content, Some(failure))
    }
    pub(super) fn preview_failure_for_head(
        &self,
        index: usize,
        worker_owned: bool,
        detail: Option<crate::LiveRendererScanoutBufferExportDetail>,
    ) -> Option<LivePreviewFrameFailure> {
        let detail = detail.filter(|detail| recoverable(*detail))?;
        let head = &self.heads[index];
        let (content, rendered) = if worker_owned {
            (head.rendering_content?, head.output_frames.rendering()?)
        } else {
            (head.pending_content?, head.output_frames.pending()?)
        };
        let list = &rendered.snapshot.compositor_display_list;
        let stamp = list.presentation_stamp()?;
        let identity = self.native_frame_identity(index, head.output.id, content.frame());
        let source = *self.preview_images.frame_sources.get(&identity)?;
        if !list
            .surface_instances()
            .any(|instance| instance.source == source)
        {
            return None;
        }
        Some(LivePreviewFrameFailure {
            output: head.output.id,
            frame: content.frame(),
            owner_epoch: stamp.owner_epoch,
            publication: stamp.publication_generation,
            source,
            required_retirement: content.requires_retirement(),
            detail,
            recorded_at: Instant::now(),
            recovery_started_at: None,
        })
    }

    pub(crate) fn preview_frame_failures(&mut self) -> Vec<LivePreviewFrameFailure> {
        begin_recoveries(&mut self.preview_failures, Instant::now())
    }
    /// Detach already settles client obligations and re-arms exact shell claims.
    /// These frames cannot be retried after the native owners have detached.
    pub(crate) fn take_detached_preview_failures(&mut self) -> Vec<LivePreviewFrameFailure> {
        take_detached_failures(&mut self.preview_failures)
    }
    pub(crate) fn has_preview_frame_failure(&self, output: OutputId) -> bool {
        self.preview_failures.contains_key(&output)
    }
    pub(crate) fn finish_preview_frame_recovery(
        &mut self,
        failure: LivePreviewFrameFailure,
    ) -> Result<(), &'static str> {
        if self.preview_failures.get(&failure.output) != Some(&failure) {
            return Err("preview recovery lost its exact failure owner");
        }
        self.preview_failures.remove(&failure.output);
        Ok(())
    }

    /// Poll failed-generation renders and cancel prepared owners without
    /// touching any submitted generation. false means cleanup still owns work.
    pub(crate) fn withdraw_preview_frame(
        &mut self,
        failure: LivePreviewFrameFailure,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        failure.check_deadline(Instant::now())?;
        let indices = self.head_indices(failure.output);
        if indices.iter().any(|index| {
            self.heads[*index]
                .submitted_content
                .is_some_and(|content| content.frame() == failure.frame)
        }) {
            return Err("preview recovery cannot withdraw a submitted frame".into());
        }
        let mut ready = true;
        for index in indices {
            if self.heads[index].prepared_group_frame == Some(failure.frame)
                && let Some(prepared) = self.heads[index].prepared_scanout.take()
                && !self.cancel_prepared_head_owner(index, prepared)
            {
                ready = false;
            }
            if self.heads[index]
                .rendering_content
                .is_some_and(|c| c.frame() == failure.frame)
            {
                let expected = self.native_frame_identity(index, failure.output, failure.frame);
                if !self.exporters[index].poll_discarded_preview_render(expected)? {
                    ready = false;
                    continue;
                }
                self.heads[index].rendering_content = None;
                self.heads[index].output_frames.discard_rendering();
            }
            if self.heads[index]
                .pending_content
                .is_some_and(|c| c.frame() == failure.frame)
            {
                if !self.exporters[index].worker_in_flight() {
                    self.exporters[index].discard_pending_frame();
                    self.heads[index].pending_content = None;
                    self.heads[index].output_frames.discard_pending();
                } else {
                    ready = false;
                }
            }
            let group = self.heads[index].group;
            self.heads[index]
                .scanout_custody
                .retry_cleanup(self.groups[group].session.card());
            ready &= !self.heads[index].scanout_custody.cleanup_pending();
        }
        // A lagging mirror sibling can still own a protected older frame.
        // Its flip must drain before a new retirement can be admitted.
        ready &= !self.installed_retirement_protected(failure.output);
        if ready {
            self.output_cohorts.remove(&(failure.output, failure.frame));
        }
        Ok(ready)
    }
}

fn take_detached_failures(
    failures: &mut BTreeMap<OutputId, LivePreviewFrameFailure>,
) -> Vec<LivePreviewFrameFailure> {
    std::mem::take(failures).into_values().collect()
}

fn record_failure(
    failures: &mut BTreeMap<OutputId, LivePreviewFrameFailure>,
    failure: LivePreviewFrameFailure,
) -> Result<(), &'static str> {
    match failures.entry(failure.output) {
        std::collections::btree_map::Entry::Vacant(entry) => {
            entry.insert(failure);
            Ok(())
        }
        std::collections::btree_map::Entry::Occupied(_) => {
            Err("preview recovery already owns this output")
        }
    }
}

fn begin_recoveries(
    failures: &mut BTreeMap<OutputId, LivePreviewFrameFailure>,
    now: Instant,
) -> Vec<LivePreviewFrameFailure> {
    failures
        .values_mut()
        .map(|failure| {
            // Owner scheduling before the first attempt is not a hung worker.
            // Subsequent attempts retain this deadline, including mirror waits.
            failure.recovery_started_at.get_or_insert(now);
            *failure
        })
        .collect()
}

fn recovery_blocks(
    failures: &BTreeMap<OutputId, LivePreviewFrameFailure>,
    output: OutputId,
    now: Instant,
) -> Result<bool, crate::LiveRendererScanoutBufferExportDetail> {
    match failures.get(&output) {
        Some(failure) => {
            failure.check_deadline(now)?;
            Ok(true)
        }
        None => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/support/preview_recovery_state.rs"
    ));
}
