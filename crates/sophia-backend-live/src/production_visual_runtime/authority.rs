use super::*;

pub(super) struct LiveProductionPreparedAuthorityRun {
    pub(super) report: crate::LiveBackendRuntimeTickReport,
    pub(super) primary_logical_target: Option<LiveProductionCpuTarget>,
}

fn replaceable_deferred_present_surface(group: &LiveProductionAuthorityGroup) -> Option<SurfaceId> {
    let [submission] = group.present_submissions.as_slice() else {
        return None;
    };
    if submission.layout_disposition != LiveProductionPresentDisposition::Immediate
        || !group.cpu_buffer_updates.is_empty()
        || !group.removed_surfaces.is_empty()
        || !group.software_present_submissions.is_empty()
    {
        return None;
    }
    let [transaction] = group.transactions.as_slice() else {
        return None;
    };
    (transaction.transaction == submission.transaction
        && transaction.surface == submission.surface
        && transaction.target_buffer()
            == (BufferSource::DmaBuf {
                handle: submission.buffer.raw(),
            }))
    .then_some(submission.surface)
}

impl LiveProductionVisualRuntime {
    /// Whether retirement released ordered content that needs another cycle.
    pub fn has_released_surface_content(&self) -> bool {
        !self.released_surface_content.is_empty()
    }

    pub fn released_surface_content_requires_gpu(&self) -> bool {
        self.released_surface_content
            .iter()
            .any(|group| !group.present_submissions.is_empty())
    }

    pub fn released_surface_content_transaction(&self) -> Option<TransactionId> {
        self.released_surface_content
            .front()
            .map(|group| group.transaction)
    }

    pub(super) fn ready_surface_content_batch(
        &mut self,
        batch: &LiveProductionAuthorityBatch,
    ) -> Result<LiveProductionAuthorityBatch, Box<dyn std::error::Error>> {
        batch.validate()?;
        let mut ordered = self.released_surface_content.drain(..).collect::<Vec<_>>();
        ordered.extend(batch.groups.iter().cloned());
        let ordered =
            rebase_authority_groups_to_committed(ordered, self.production.committed_surfaces());
        let mut groups = Vec::with_capacity(ordered.len());
        for group in ordered {
            let touched = group
                .transactions
                .iter()
                .map(|transaction| transaction.surface)
                .collect::<Vec<_>>();
            let removed = group.removed_surfaces.clone();
            let replaceable_surface = replaceable_deferred_present_surface(&group);
            let admission = match replaceable_surface {
                Some(surface) => self.surface_content_stream.admit_latest_deferred(
                    group,
                    touched,
                    removed,
                    |deferred| replaceable_deferred_present_surface(deferred) == Some(surface),
                )?,
                None => self.surface_content_stream.admit(group, touched, removed)?,
            };
            match admission {
                SurfaceContentAdmission::Ready(group) => {
                    for owner in authority_group_present_owners(&group)? {
                        self.surface_content_stream.begin(owner)?;
                    }
                    groups.push(group);
                }
                SurfaceContentAdmission::Deferred { superseded } => {
                    if let Some(superseded) = superseded {
                        self.superseded_surface_content.push_back(superseded);
                    }
                }
            }
        }
        Ok(LiveProductionAuthorityBatch {
            groups,
            dma_buf_registrations: batch.dma_buf_registrations.clone(),
            fence_registrations: batch.fence_registrations.clone(),
            released_dma_bufs: batch.released_dma_bufs.clone(),
            released_fences: batch.released_fences.clone(),
        })
    }

    pub(super) fn finish_surface_content_owner(
        &mut self,
        owner: SurfaceTransactionKey,
    ) -> Result<usize, Box<dyn std::error::Error>> {
        let ready = self.surface_content_stream.finish(owner)?;
        let count = ready.len();
        self.released_surface_content.extend(ready);
        Ok(count)
    }

    fn finish_surface_content_transaction(
        &mut self,
        transaction: TransactionId,
    ) -> Result<usize, Box<dyn std::error::Error>> {
        let Some(owner) = self
            .surface_content_stream
            .owner_for_transaction(transaction)
        else {
            return Ok(0);
        };
        self.finish_surface_content_owner(owner)
    }

    pub(super) fn observe_content_ordered_resource_releases(
        &mut self,
        batch: &LiveProductionAuthorityBatch,
    ) {
        for handle in &batch.released_dma_bufs {
            if self.pending_surface_content_references_dma_buf(*handle) {
                self.deferred_content_dma_buf_releases.insert(*handle);
            } else {
                let _ = self
                    .presentation_feedback
                    .resources_mut()
                    .release_source(*handle);
            }
        }
        for handle in &batch.released_fences {
            if self.pending_surface_content_references_fence(*handle) {
                self.deferred_content_fence_releases.insert(*handle);
            } else {
                let _ = self
                    .presentation_feedback
                    .resources_mut()
                    .release_fence(*handle);
            }
        }

        let dma_bufs = self
            .deferred_content_dma_buf_releases
            .iter()
            .filter(|handle| !self.pending_surface_content_references_dma_buf(**handle))
            .copied()
            .collect::<Vec<_>>();
        for handle in dma_bufs {
            self.deferred_content_dma_buf_releases.remove(&handle);
            let _ = self
                .presentation_feedback
                .resources_mut()
                .release_source(handle);
        }
        let fences = self
            .deferred_content_fence_releases
            .iter()
            .filter(|handle| !self.pending_surface_content_references_fence(**handle))
            .copied()
            .collect::<Vec<_>>();
        for handle in fences {
            self.deferred_content_fence_releases.remove(&handle);
            let _ = self
                .presentation_feedback
                .resources_mut()
                .release_fence(handle);
        }
    }

    fn pending_surface_content_references_dma_buf(&self, handle: BufferHandle) -> bool {
        self.surface_content_stream
            .deferred_items()
            .chain(self.released_surface_content.iter())
            .chain(self.superseded_surface_content.iter())
            .flat_map(|group| group.present_submissions.iter())
            .any(|submission| submission.buffer == handle)
    }

    fn pending_surface_content_references_fence(&self, handle: FenceHandle) -> bool {
        self.surface_content_stream
            .deferred_items()
            .chain(self.released_surface_content.iter())
            .chain(self.superseded_surface_content.iter())
            .any(|group| {
                group.present_submissions.iter().any(|submission| {
                    submission.acquire_fence == Some(handle)
                        || submission.idle_fence == Some(handle)
                }) || group.software_present_submissions.iter().any(|submission| {
                    submission.acquire_fence == Some(handle)
                        || submission.idle_fence == Some(handle)
                })
            })
    }

    pub(super) fn enqueue_software_presents(
        &mut self,
        groups: &[LiveProductionAuthorityGroup],
    ) -> Result<(), Box<dyn std::error::Error>> {
        let submissions = groups
            .iter()
            .flat_map(|group| group.software_present_submissions.iter().copied())
            .collect::<Vec<_>>();
        if submissions.is_empty() {
            return Ok(());
        }
        for submission in &submissions {
            self.presentation_feedback.resources_mut().begin_software(
                submission.transaction,
                submission.acquire_fence,
                submission.idle_fence,
            )?;
            if !self
                .presentation_feedback
                .resources_mut()
                .poll_acquire_fence(submission.transaction)?
            {
                return Err("software Present acquire fence is not ready".into());
            }
        }
        self.software_presents_unframed.push_back(submissions);
        Ok(())
    }

    pub fn drain_retired_software_presents_into(
        &mut self,
        retired: &mut Vec<LiveProductionRetiredSoftwarePresent>,
    ) -> Result<(), &'static str> {
        if self.retired_software_presents_overflowed {
            return Err("production software Present retirement queue overflowed");
        }
        retired.extend(self.retired_software_presents.drain(..));
        Ok(())
    }

    pub(super) fn reject_software_presents(&mut self) -> usize {
        self.reject_software_present_frames()
    }

    /// Settles software presents whose head frames a topology transition threw
    /// away.
    ///
    /// A bound software present is retired only by a real page flip of the head
    /// frames it was lowered onto. Topology installation and abort discard
    /// exactly those frames, so without this the binding would wait on a flip
    /// that can never happen: the runtime would never report quiescent again
    /// and the client would never receive its feedback. Returns how many were
    /// settled; zero is the ordinary case.
    pub fn settle_discarded_software_presents(&mut self) -> usize {
        self.reject_software_present_frames()
    }

    pub(super) fn reject_superseded_surface_content(
        &mut self,
    ) -> Result<usize, Box<dyn std::error::Error>> {
        let mut rejected = 0usize;
        while let Some(group) = self.superseded_surface_content.pop_front() {
            rejected = rejected.saturating_add(self.reject_unstarted_present_group(&group)?);
        }
        Ok(rejected)
    }

    fn reject_unstarted_present_group(
        &mut self,
        group: &LiveProductionAuthorityGroup,
    ) -> Result<usize, Box<dyn std::error::Error>> {
        let mut rejected = 0usize;
        for submission in &group.present_submissions {
            self.presentation_feedback.resources_mut().begin(
                crate::LivePresentationSubmission {
                    transaction: submission.transaction,
                    buffer: submission.buffer,
                    acquire_fence: submission.acquire_fence,
                    idle_fence: submission.idle_fence,
                },
            )?;
            let outcome = self
                .presentation_feedback
                .reject_skip_at_last_display(submission.transaction)?;
            self.route_present_feedback(outcome);
            rejected = rejected.saturating_add(1);
            self.present_rejections = self.present_rejections.saturating_add(1);
        }
        for submission in &group.software_present_submissions {
            self.presentation_feedback.resources_mut().begin_software(
                submission.transaction,
                submission.acquire_fence,
                submission.idle_fence,
            )?;
            let outcome = self
                .presentation_feedback
                .reject_skip_at_last_display(submission.transaction)?;
            self.route_present_feedback(outcome);
            rejected = rejected.saturating_add(1);
            self.present_rejections = self.present_rejections.saturating_add(1);
        }
        Ok(rejected)
    }

    pub fn reject_gpu_presentation(&mut self, transaction: TransactionId) -> bool {
        let rejected = if let Ok(outcome) = self
            .presentation_feedback
            .reject_skip_at_last_display(transaction)
        {
            self.route_present_feedback(outcome);
            self.present_rejections = self.present_rejections.saturating_add(1);
            true
        } else {
            false
        };
        match self.finish_surface_content_transaction(transaction) {
            Ok(released) if released != 0 => tracing::debug!(
                transaction = transaction.raw(),
                groups = released,
                "rejected Present released ordered surface content"
            ),
            Ok(_) => {}
            Err(error) => tracing::error!(
                transaction = transaction.raw(),
                %error,
                "failed to release rejected Present content owner"
            ),
        }
        rejected
    }

    pub fn release_layout_deferred_presentations(&mut self) {
        // `presentation_order` is the last projection applied at the Engine
        // boundary. It deliberately lags pre-admission recovery state until
        // the CPU snapshot transaction makes the surface scene-visible.
        let report = self.present_scheduler.release_layout_deferred_for_surfaces(
            &self.presentation_order,
            self.production.committed_surfaces(),
        );
        for transaction in report.superseded {
            self.reject_gpu_presentation(transaction);
        }
    }

    /// Skip a Present that reached production without an admission claim.
    ///
    /// A client that presents before mapping leaves a frame production owns and
    /// admission has no record of. It cannot become visible and nothing will
    /// settle it, so the sooner it takes the ordinary rejection the sooner the
    /// client gets its buffer back and can draw a frame that *can* be admitted.
    /// Returns whether the candidate was still queued to skip.
    /// `None` when the candidate is not queued here, which is not terminal:
    /// intake may not have reached it yet, and the caller should keep asking.
    /// `Some(settled)` once it has been removed, where `settled` reports only
    /// that the rejection produced and queued its feedback. Routing and writing
    /// it to the client happen later, so this is a backend result and not proof
    /// the client was told anything; the routed Complete and Idle records are
    /// what show delivery.
    pub fn skip_escaped_pre_admission(
        &mut self,
        key: sophia_protocol::DmaBufPresentKey,
    ) -> Option<bool> {
        let transaction = self.present_scheduler.remove_queued_dma_candidate(key)?;
        // Reported at info with the exact identity, because the next real-client
        // run has to show this path ran. Inferring it from an absent expiry
        // record would confuse "skipped promptly" with "never parked".
        let settled = self.reject_gpu_presentation(transaction);
        tracing::info!(
            transaction = transaction.raw(),
            surface = key.surface.index(),
            buffer = key.buffer.raw(),
            settled,
            "sophia_live_pre_admission_skip schema=1 status=skipped"
        );
        Some(settled)
    }

    /// Service the first candidates parked by the Present path.
    ///
    /// Separate from the layout-deferred release above because that one only
    /// runs while no layout is pending, and admission is exactly what is
    /// pending while a first frame is waited on. Releasing parked first
    /// candidates only when nothing is pending cannot end a wait an admission
    /// attempt began.
    pub fn service_first_visibility_presentations(&mut self, now: std::time::Instant) {
        let time = self.translation_time();
        let visible = self
            .present_scheduler
            .awaiting_first_visibility()
            .filter_map(|(surface, geometry, reason)| {
                // Each candidate is released on the condition that parked it.
                // Testing every one against visibility would leave a candidate
                // parked for absence from the presentation order waiting on a
                // stricter condition than the one it failed.
                let released = match reason {
                    crate::LiveProductionFirstVisibilityReason::OutsidePresentationOrder => {
                        self.presentation_order.contains(&surface)
                    }
                    crate::LiveProductionFirstVisibilityReason::NoApplicableOutput
                    | crate::LiveProductionFirstVisibilityReason::OutsideHeadFrames => {
                        self.outputs.logical_viewports().any(|(output, viewport)| {
                            live_surface_routes_to_output(
                                surface,
                                &self.surface_outputs,
                                &self.geometry_routed_surfaces,
                                output,
                            ) && !crate::presentation::intersect_rects(
                                self.translations.geometry(surface, output, geometry, time),
                                viewport,
                            )
                            .is_empty()
                        })
                    }
                };
                released.then_some(surface)
            })
            .collect::<Vec<_>>();
        self.present_scheduler.release_first_visibility(&visible);
        for (surface, reason) in self.present_scheduler.expire_first_visibility(now) {
            // A first candidate that waits silently is indistinguishable from
            // one that is merely slow, which is what made this expensive to
            // find the first time.
            tracing::warn!(
                surface = surface.index(),
                ?reason,
                "first-visibility budget expired; candidate returned for ordinary rejection"
            );
        }
    }

    pub fn commit_layout_epoch(&mut self, epoch: TransactionId) -> usize {
        self.present_scheduler.commit_layout_epoch(epoch)
    }

    pub fn abort_layout_epoch(
        &mut self,
        epoch: TransactionId,
    ) -> crate::LiveProductionLayoutRollbackReport {
        let report = self.present_scheduler.abort_layout_epoch(epoch);
        for transaction in &report.rejected {
            let transaction = *transaction;
            self.reject_gpu_presentation(transaction);
        }
        report
    }

    pub fn route_present_feedback(&mut self, outcome: crate::LivePresentFeedbackOutcome) {
        if self.present_feedback.len() == PRESENT_FEEDBACK_CAPACITY {
            self.present_feedback_overflowed = true;
            return;
        }
        self.present_feedback.push_back(outcome);
    }

    pub(super) fn release_removed_presentations(
        &mut self,
        removed_surfaces: &[SurfaceId],
        mut native_scanout: Option<&mut LiveProductionNativeScanout>,
    ) -> Result<(), crate::LiveRendererScanoutBufferExportDetail> {
        for surface in removed_surfaces {
            if let Some(displayed) = self.displayed_surfaces.remove(surface)
                && let Some(native) = native_scanout.as_deref_mut()
            {
                native.evict_renderer_image(displayed.layer.image_id)?;
            }
        }
        Ok(())
    }

    pub fn drain_present_feedback_into(
        &mut self,
        outcomes: &mut Vec<crate::LivePresentFeedbackOutcome>,
    ) -> Result<(), &'static str> {
        if self.present_feedback_overflowed {
            return Err("production Present feedback queue overflowed");
        }
        outcomes.extend(self.present_feedback.drain(..));
        Ok(())
    }

    pub fn shutdown_presentations(
        &mut self,
    ) -> Result<crate::LivePresentationDisconnectReport, Box<dyn std::error::Error>> {
        // A directly scanned buffer is held until a successor flip retires it,
        // and at shutdown there is no successor. Releasing it here is the only
        // release it will ever get; holding it instead left the session unable
        // to shut down after the first frame that ever reached a plane
        // directly. See `PresentFlipOwnership.tla`, `ReleasedOnlyBySuccessor`:
        // the screen is going away, which retires it as surely as a flip does.
        self.release_displayed_direct_presents();
        let mut shutdown_rejections = self.reject_software_presents();
        let queued = self.present_scheduler.drain_transactions();
        for transaction in queued {
            shutdown_rejections = shutdown_rejections
                .saturating_add(usize::from(self.reject_gpu_presentation(transaction)));
        }
        if let Some(submitted) = self.present_scheduler.take_submitted() {
            shutdown_rejections = shutdown_rejections.saturating_add(usize::from(
                self.reject_gpu_presentation(submitted.transaction),
            ));
        }
        if let Some(rendering) = self.present_scheduler.take_rendering() {
            shutdown_rejections = shutdown_rejections.saturating_add(usize::from(
                self.reject_gpu_presentation(rendering.transaction),
            ));
        }
        // These rejections already belong to the supersession counter even
        // when shutdown happens before the next owner cycle routes them.
        let _ = self.reject_superseded_surface_content()?;
        let mut deferred = self.released_surface_content.drain(..).collect::<Vec<_>>();
        deferred.extend(self.surface_content_stream.drain_deferred());
        let discarded = deferred.len();
        for group in deferred {
            shutdown_rejections =
                shutdown_rejections.saturating_add(self.reject_unstarted_present_group(&group)?);
        }
        if self.surface_content_stream.active_len() != 0 {
            return Err("presentation shutdown retained active surface content ownership".into());
        }
        let _ = self.surface_content_stream.discard();
        self.deferred_content_dma_buf_releases.clear();
        self.deferred_content_fence_releases.clear();
        if discarded != 0 {
            tracing::debug!(
                deferred_groups = discarded,
                "discarded fenced surface authority during presentation shutdown"
            );
        }
        self.displayed_surfaces.clear();
        self.shutdown_present_rejections = self
            .shutdown_present_rejections
            .saturating_add(shutdown_rejections);

        Ok(self.presentation_feedback.disconnect())
    }

    pub fn prepare_authority_transactions(
        &mut self,
        transaction_id: TransactionId,
        transactions: &[SurfaceTransaction],
        removed_surfaces: &[SurfaceId],
    ) -> Result<LiveProductionPreparedAuthorityBatch, Box<dyn std::error::Error>> {
        self.observe_surface_metadata(transactions, removed_surfaces);
        let intake = AuthorityTransactionIntake::new(transaction_id, transactions.to_vec())
            .with_surface_removals(removed_surfaces.to_vec());
        let authority_commits = self
            .production
            .commit_authority_batches(std::slice::from_ref(&intake));
        Ok(LiveProductionPreparedAuthorityBatch {
            authority_commits,
            layer_templates: self.compositor_layer_templates(),
        })
    }

    pub fn prepare_authority_groups(
        &mut self,
        groups: &[LiveProductionAuthorityGroup],
    ) -> Result<LiveProductionPreparedAuthorityBatch, Box<dyn std::error::Error>> {
        let mut intakes = Vec::with_capacity(groups.len());
        for group in groups {
            group.validate()?;
            self.observe_surface_metadata(&group.transactions, &group.removed_surfaces);
            intakes.push(
                AuthorityTransactionIntake::new(group.transaction, group.transactions.clone())
                    .with_surface_removals(group.removed_surfaces.clone()),
            );
        }
        let authority_commits = self.production.commit_authority_batches(&intakes);
        Ok(LiveProductionPreparedAuthorityBatch {
            authority_commits,
            layer_templates: self.compositor_layer_templates(),
        })
    }

    pub(super) fn run_prepared_authority_transactions_with_targets(
        &mut self,
        prepared: LiveProductionPreparedAuthorityBatch,
        event_count: usize,
        mut native_scanout: Option<&mut LiveProductionNativeScanout>,
        native_head_frames: Option<Vec<(OutputId, Vec<crate::LiveProductionHeadCompositionFrame>)>>,
        wm_update: Option<WmTransactionUpdate>,
    ) -> Result<LiveProductionPreparedAuthorityRun, Box<dyn std::error::Error>> {
        let native_enabled = native_scanout.is_some();
        let output_count = self.outputs.output_count();
        let production = &self.production;
        let primary_output = self.outputs.primary_output();
        let ordinary_repaints_pending = &mut self.ordinary_repaints_pending;
        let outputs = &mut self.outputs;
        let surface_metadata = &self.surface_metadata;
        let native_head_frames_requested = native_head_frames.is_some();
        let provided_native_head_frames = native_head_frames.unwrap_or_default();
        let provided_native_output_count = provided_native_head_frames.len();
        let mut native_head_frames = provided_native_head_frames
            .into_iter()
            .collect::<BTreeMap<_, _>>();
        if native_head_frames.len() != provided_native_output_count {
            return Err("native head composition named a logical output more than once".into());
        }
        if native_head_frames_requested && native_head_frames.len() != output_count {
            return Err("native head composition did not cover every logical output".into());
        }
        if native_head_frames_requested && !native_enabled {
            return Err("native head composition was provided without native scanout".into());
        }
        let primary_logical_target = std::cell::Cell::new(None);
        let primary_logical_target_ref = &primary_logical_target;
        let mut adapter = crate::LiveProductionOutputRuntimeAdapter::new(
            output_count,
            |index, committed: &[CommittedSurfaceState]| -> Result<_, Box<dyn std::error::Error>> {
                // The adapter counts logical outputs, and every native entry point
                // is addressed by output identity rather than by that count.
                let output_id = outputs
                    .output_id(index)
                    .ok_or("production output index was not registered")?;
                // Templates from the same slice this closure just replaced into
                // the assembly, not from prepare time. Any commit landing between
                // prepare and run -- a Present settling, a retirement -- would
                // otherwise leave the assembly holding a surface the frozen
                // templates lack, which the engine rejects as an invalid surface.
                let input = compositor_tick_input_for_committed(
                    committed,
                    surface_metadata,
                    event_count,
                    prepared.authority_commits.clone(),
                    wm_update.clone(),
                );
                Ok(match native_scanout.as_deref_mut() {
                    Some(native_scanout) => {
                        if let Some(frames) = native_head_frames.remove(&output_id) {
                            if outputs.native_initialized(output_id) {
                                let Some(logical_checksum) =
                                    frames.first().map(|frame| frame.logical_content_checksum)
                                else {
                                    return Err("native head composition is empty".into());
                                };
                                if frames
                                    .iter()
                                    .any(|frame| frame.logical_content_checksum != logical_checksum)
                                {
                                    return Err(
                                        "native heads disagree on logical content checksum".into(),
                                    );
                                }
                                let frame = ordinary_repaint::admit(
                                    ordinary_repaints_pending,
                                    native_scanout,
                                    output_id,
                                    frames,
                                )?;
                                if Some(output_id) == primary_output {
                                    primary_logical_target_ref.set(frame.map(|frame| {
                                        LiveProductionCpuTarget::new(frame, logical_checksum)
                                    }));
                                }
                            } else {
                                outputs.initialize_native_head_composition(
                                    native_scanout,
                                    output_id,
                                    frames,
                                )?;
                            }
                        }
                        outputs.run_output(index, committed, |runtime| {
                            Ok(if runtime.rendered_primary_plane_scanout_in_flight() {
                                runtime.run_tick(input)?
                            } else {
                                native_scanout.run_tick(output_id, runtime, input)?
                            })
                        })?
                    }
                    None => outputs
                        .run_output(index, committed, |runtime| Ok(runtime.run_tick(input)?))?,
                })
            },
        );
        let report = production
            .run_outputs(&mut adapter)?
            .into_iter()
            .next()
            .ok_or("persistent backend runtime has no outputs")?;
        // Ends the borrow rather than releasing a resource: the adapter owns
        // nothing that needs dropping here.
        let _ = adapter;
        if !native_head_frames.is_empty() {
            return Err("native head composition named an unknown logical output".into());
        }
        let primary_logical_target = primary_logical_target.get();
        if !native_enabled {
            self.publish_committed_input_layers();
        }
        Ok(LiveProductionPreparedAuthorityRun {
            report,
            primary_logical_target,
        })
    }

    pub fn run_prepared_authority_transactions(
        &mut self,
        prepared: LiveProductionPreparedAuthorityBatch,
        event_count: usize,
        native_scanout: Option<&mut LiveProductionNativeScanout>,
        native_head_frames: Option<Vec<(OutputId, Vec<crate::LiveProductionHeadCompositionFrame>)>>,
        wm_update: Option<WmTransactionUpdate>,
    ) -> Result<crate::LiveBackendRuntimeTickReport, Box<dyn std::error::Error>> {
        self.run_prepared_authority_transactions_with_targets(
            prepared,
            event_count,
            native_scanout,
            native_head_frames,
            wm_update,
        )
        .map(|run| run.report)
    }

    pub fn run_authority_transactions(
        &mut self,
        run: LiveAuthorityTransactionRun<'_>,
    ) -> Result<crate::LiveBackendRuntimeTickReport, Box<dyn std::error::Error>> {
        let LiveAuthorityTransactionRun {
            groups,
            event_count,
            native_scanout,
            native_head_frames,
            wm_update,
        } = run;
        let prepared = self.prepare_authority_groups(groups)?;
        self.run_prepared_authority_transactions(
            prepared,
            event_count,
            native_scanout,
            native_head_frames,
            wm_update,
        )
    }

    pub fn committed_surfaces(&self) -> &[CommittedSurfaceState] {
        self.production.committed_surfaces()
    }

    pub fn input_layers(&self) -> &[LayerSnapshot] {
        self.input_projections
            .first()
            .map_or(&[], |projection| projection.layers.as_slice())
    }

    pub fn input_presentation_epoch(&self) -> u64 {
        self.input_projections
            .first()
            .map_or(0, |projection| projection.epoch)
    }

    pub fn input_output(&self) -> Option<OutputId> {
        self.outputs.primary_output()
    }

    pub fn input_projections(&self) -> &[LivePresentedInputProjection] {
        &self.input_projections
    }
}
