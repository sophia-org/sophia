use super::*;

mod layout_witness;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LiveProductionNativeSuspendOutcome {
    #[default]
    Drained,
    ForcedDetachTimeout,
    ForcedDetachDrainError,
    ForcedDetachRevoked,
}

impl LiveProductionNativeSuspendOutcome {
    pub const fn reduced_name(self) -> &'static str {
        match self {
            Self::Drained => "drained",
            Self::ForcedDetachTimeout => "forced_detach_timeout",
            Self::ForcedDetachDrainError => "forced_detach_drain_error",
            Self::ForcedDetachRevoked => "forced_detach_revoked",
        }
    }

    pub const fn drained(self) -> bool {
        matches!(self, Self::Drained)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LiveProductionNativeSuspendReport {
    pub outcome: LiveProductionNativeSuspendOutcome,
    pub abandoned_scanouts: usize,
    pub skipped_present: Option<TransactionId>,
}

/// What a topology escalation had to skip to reach quiescence.
///
/// Every count here is a present a client will never see. Zero across all
/// three is the ordinary case and means the wait converged on its own.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LiveTopologyPresentationSkipReport {
    pub skipped_in_flight: Option<TransactionId>,
    pub skipped_queued: usize,
    pub skipped_software: usize,
}

impl LiveTopologyPresentationSkipReport {
    /// Whether anything was actually given up.
    pub const fn is_empty(&self) -> bool {
        self.skipped_in_flight.is_none() && self.skipped_queued == 0 && self.skipped_software == 0
    }
}

#[derive(Debug)]
pub struct LiveProductionNativeSuspendError {
    pub drain_error: Box<dyn std::error::Error>,
    pub detach_report: Option<LiveProductionNativeSuspendReport>,
    pub detach_error: Option<Box<dyn std::error::Error>>,
}

impl LiveProductionNativeSuspendError {
    pub const fn forced_detach_established(&self) -> bool {
        self.detach_report.is_some()
    }
}

impl std::fmt::Display for LiveProductionNativeSuspendError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "native scanout drain failed: {}",
            self.drain_error
        )?;
        if let Some(report) = self.detach_report {
            write!(
                formatter,
                "; forced detach completed: outcome={} abandoned_scanouts={} skipped_present={}",
                report.outcome.reduced_name(),
                report.abandoned_scanouts,
                report.skipped_present.map_or_else(
                    || "none".to_owned(),
                    |transaction| transaction.raw().to_string()
                )
            )
        } else if let Some(error) = self.detach_error.as_deref() {
            write!(formatter, "; forced detach failed: {error}")
        } else {
            write!(formatter, "; forced detach outcome is unknown")
        }
    }
}

impl std::error::Error for LiveProductionNativeSuspendError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.drain_error.as_ref())
    }
}

/// Complete a native suspension after its bounded drain attempt.
///
/// This seam keeps forced-detach ordering testable without constructing real
/// DRM resources: every drain failure invokes `detach` before the original
/// error is returned.
pub fn finish_live_production_native_suspend(
    drain: Result<bool, Box<dyn std::error::Error>>,
    detach: impl FnOnce(
        LiveProductionNativeSuspendOutcome,
    ) -> Result<LiveProductionNativeSuspendReport, Box<dyn std::error::Error>>,
) -> Result<LiveProductionNativeSuspendReport, Box<dyn std::error::Error>> {
    match drain {
        Ok(true) => detach(LiveProductionNativeSuspendOutcome::Drained),
        Ok(false) => detach(LiveProductionNativeSuspendOutcome::ForcedDetachTimeout),
        Err(drain_error) => {
            match detach(LiveProductionNativeSuspendOutcome::ForcedDetachDrainError) {
                Ok(detach_report) => Err(Box::new(LiveProductionNativeSuspendError {
                    drain_error,
                    detach_report: Some(detach_report),
                    detach_error: None,
                })),
                Err(detach_error) => Err(Box::new(LiveProductionNativeSuspendError {
                    drain_error,
                    detach_report: None,
                    detach_error: Some(detach_error),
                })),
            }
        }
    }
}

fn validate_renderer_image_resume_admission(
    retained: &[LiveRendererImageId],
    handoff: Option<&[LiveRendererImageId]>,
) -> Result<(), &'static str> {
    match crate::reduce_live_renderer_image_handoff_admission(retained, handoff) {
        crate::LiveRendererImageHandoffAdmission::Ready => Ok(()),
        crate::LiveRendererImageHandoffAdmission::Missing => {
            Err("native resume omitted retained renderer images")
        }
        crate::LiveRendererImageHandoffAdmission::InvalidIdentity => {
            Err("native resume contains an invalid renderer-image identity")
        }
        crate::LiveRendererImageHandoffAdmission::DuplicateIdentity => {
            Err("native resume contains a duplicate renderer-image identity")
        }
        crate::LiveRendererImageHandoffAdmission::CoverageMismatch => {
            Err("native resume renderer-image handoff does not match the retained scene")
        }
    }
}

fn advance_renderer_image_resume(
    phase: crate::LiveRendererImageResumePhase,
    observation: crate::LiveRendererImageResumeObservation,
) -> Result<crate::LiveRendererImageResumePhase, &'static str> {
    match crate::reduce_live_renderer_image_resume_observation(phase, observation) {
        crate::LiveRendererImageResumeTransition::Advanced(next) => Ok(next),
        crate::LiveRendererImageResumeTransition::Rejected => {
            Err("native resume renderer-image lifecycle is out of order")
        }
    }
}

pub const fn reduce_live_production_abandoned_scanout_count(
    logical_runtime_owners: usize,
    physical_head_owners: usize,
) -> usize {
    logical_runtime_owners.saturating_add(physical_head_owners)
}

impl LiveProductionVisualRuntime {
    pub fn shell_content_presentation_available(
        &self,
        native_scanout: &LiveProductionNativeScanout,
    ) -> bool {
        !self.native_suspended && native_scanout.output_topology_allows_frame_service()
    }

    pub fn retained_renderer_image_ids(&self) -> Vec<LiveRendererImageId> {
        let mut images = self
            .displayed_surfaces
            .values()
            .map(|displayed| displayed.layer.image_id)
            .collect::<BTreeSet<_>>();
        if let Some((_, in_flight)) = self.present_scheduler.in_flight_displayed_layer() {
            images.insert(in_flight.image_id);
        }
        images.into_iter().collect()
    }

    pub fn discard_retained_renderer_images(&mut self) -> usize {
        let discarded = self.displayed_surfaces.len();
        self.displayed_surfaces.clear();
        discarded
    }

    pub fn suspend_native_scanout(
        &mut self,
        native_scanout: &mut LiveProductionNativeScanout,
        outputs: &[sophia_engine::HeadlessOutput],
        timeout: Duration,
    ) -> Result<LiveProductionNativeSuspendReport, Box<dyn std::error::Error>> {
        let drain = self.drain_native_scanout_until(native_scanout, timeout);
        finish_live_production_native_suspend(drain, |outcome| {
            self.detach_native_scanout(Some(native_scanout), outputs, outcome)
        })
    }

    pub fn suspend_revoked_native_scanout(
        &mut self,
        outputs: &[sophia_engine::HeadlessOutput],
    ) -> Result<LiveProductionNativeSuspendReport, Box<dyn std::error::Error>> {
        self.detach_native_scanout(
            None,
            outputs,
            LiveProductionNativeSuspendOutcome::ForcedDetachRevoked,
        )
    }

    /// Skips whichever present currently owns the kernel, settling it as
    /// `Skipped` and returning its renderer image.
    ///
    /// Shared by suspend and by topology escalation so the two cannot drift:
    /// both need the same "this present will never reach a screen" settlement,
    /// and only the attributed counter differs.
    pub(super) fn skip_in_flight_present(
        &mut self,
        native_scanout: Option<&mut LiveProductionNativeScanout>,
        attribute: impl FnOnce(&mut Self),
    ) -> Option<LiveProductionSubmittedPresent> {
        let skipped = self
            .present_scheduler
            .take_submitted()
            .or_else(|| self.present_scheduler.take_rendering());
        if let Some(present) = skipped.as_ref() {
            if let Some(native_scanout) = native_scanout {
                let _ = native_scanout.rollback_renderer_image(present.displayed_layer.image_id);
            }
            if self.reject_gpu_presentation(present.transaction) {
                attribute(self);
            }
        }
        skipped
    }

    /// Forces presentation quiescence without giving up the displayed topology.
    ///
    /// The topology wait needs every present owner to settle before it may
    /// apply, but it cannot make clients stop drawing, and the owners it waits
    /// on can only advance while it is waiting. This is the escalation for
    /// that: skip what is runnable and what is waiting, settle each as
    /// `Skipped` so no client is left expecting feedback, and leave the scanout
    /// and output set alone so the current topology keeps scanning out.
    ///
    /// Layout-deferred presents are untouched. They belong to a layout epoch
    /// that will commit or abort them itself.
    pub fn skip_presentations_for_topology(
        &mut self,
        native_scanout: Option<&mut LiveProductionNativeScanout>,
    ) -> LiveTopologyPresentationSkipReport {
        let skipped_in_flight = self
            .skip_in_flight_present(native_scanout, |runtime| {
                runtime.topology_escalation_present_rejections = runtime
                    .topology_escalation_present_rejections
                    .saturating_add(1);
            })
            .map(|present| present.transaction);
        let mut skipped_queued = 0usize;
        for transaction in self.present_scheduler.drain_runnable_transactions() {
            if self.reject_gpu_presentation(transaction) {
                skipped_queued = skipped_queued.saturating_add(1);
                self.topology_escalation_present_rejections = self
                    .topology_escalation_present_rejections
                    .saturating_add(1);
            }
        }
        let skipped_software = self.reject_software_presents();
        self.topology_escalation_present_rejections = self
            .topology_escalation_present_rejections
            .saturating_add(skipped_software);
        LiveTopologyPresentationSkipReport {
            skipped_in_flight,
            skipped_queued,
            skipped_software,
        }
    }

    fn detach_native_scanout(
        &mut self,
        native_scanout: Option<&mut LiveProductionNativeScanout>,
        outputs: &[sophia_engine::HeadlessOutput],
        outcome: LiveProductionNativeSuspendOutcome,
    ) -> Result<LiveProductionNativeSuspendReport, Box<dyn std::error::Error>> {
        let abandoned_scanouts = reduce_live_production_abandoned_scanout_count(
            self.outputs.native_scanout_in_flight_count(),
            native_scanout
                .as_deref()
                .map_or(0, LiveProductionNativeScanout::head_scanout_in_flight_count),
        );
        let skipped_present = self.skip_in_flight_present(native_scanout, |runtime| {
            runtime.native_suspend_present_rejections =
                runtime.native_suspend_present_rejections.saturating_add(1);
        });
        self.reject_software_presents();
        let invalidation_epoch = self
            .input_projections
            .iter()
            .map(|projection| projection.epoch)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .expect("presented input epoch exhausted");
        self.outputs = LiveProductionOutputRuntimeSet::new(
            outputs,
            self.production.committed_surfaces(),
            None,
        )?;
        self.content_layout_generation = self
            .content_layout_generation
            .checked_add(1)
            .ok_or("content layout generation exhausted")?;
        self.translations.settle();
        self.translation_deadlines.clear();
        self.ordinary_repaints_pending.clear();
        self.native_suspended = true;
        // A suspended/revoked output no longer has a visible native
        // interaction snapshot. Do not retain routes into retired pixels.
        self.input_projections = (0..self.outputs.output_count())
            .filter_map(|index| self.outputs.output_id(index))
            .map(|output| LivePresentedInputProjection {
                output,
                epoch: invalidation_epoch,
                layers: Vec::new(),
                chrome_targets: Vec::new(),
                chrome_occlusion: None,
                descriptor_targets: Vec::new(),
                descriptor_occlusion: None,
                descriptor_projection: None,
                tab_occlusions: Vec::new(),
                content: None,
            })
            .collect();
        Ok(LiveProductionNativeSuspendReport {
            outcome,
            abandoned_scanouts,
            skipped_present: skipped_present.map(|present| present.transaction),
        })
    }

    pub fn resume_native_scanout(
        &mut self,
        native_scanout: &mut LiveProductionNativeScanout,
        outputs: &[sophia_engine::HeadlessOutput],
        scene: &LiveProductionCpuScene,
        renderer_handoff: Option<&LiveProductionRendererImageHandoff>,
    ) -> Result<usize, Box<dyn std::error::Error>> {
        let retained = self.retained_renderer_image_ids();
        validate_renderer_image_resume_admission(
            &retained,
            renderer_handoff.as_ref().map(|handoff| handoff.image_ids()),
        )?;
        let mut resume_phase = crate::LiveRendererImageResumePhase::default();
        // Build runtime-only output state first. Renderer workers and retained
        // images must exist before the semantic head plans are lowered, while
        // KMS must remain untouched until every resulting owner is prepared.
        let resumed_outputs = LiveProductionOutputRuntimeSet::new(
            outputs,
            self.production.committed_surfaces(),
            Some(native_scanout),
        )?;
        let workers = native_scanout.enable_renderer_workers()?;
        if workers != native_scanout.enabled_head_count() {
            return Err("native resume established partial renderer-worker coverage".into());
        }
        if !native_scanout.renderer_image_owners_initialized() {
            return Err("native resume did not initialize every renderer image owner".into());
        }
        resume_phase = advance_renderer_image_resume(
            resume_phase,
            crate::LiveRendererImageResumeObservation::OutputOwnerInitialized,
        )?;
        let restored = renderer_handoff.map_or(Ok(0), |handoff| {
            native_scanout.restore_renderer_image_handoff(handoff)
        })?;
        resume_phase = advance_renderer_image_resume(
            resume_phase,
            crate::LiveRendererImageResumeObservation::ImagesRestored,
        )?;
        if resume_phase != crate::LiveRendererImageResumePhase::Ready {
            return Err("native resume renderer-image lifecycle did not become ready".into());
        }
        // Install the runtime privately, lower the restored scene for every
        // native head, and synchronously present each complete output cohort.
        self.outputs = resumed_outputs;
        self.native_suspended = false;
        let batches = self.retained_output_head_composition_frames(scene, native_scanout)?;
        if batches.len() != self.outputs.output_count() {
            return Err("native resume produced partial logical-output coverage".into());
        }
        for (output, frames) in batches {
            self.outputs
                .initialize_native_head_composition(native_scanout, output, frames)?;
        }
        self.publish_presented_input_layers(native_scanout);
        Ok(restored)
    }

    /// Rebuilds logical output runtimes around scanout owners already installed
    /// by a blocking topology transaction. No renderer initialization or KMS
    /// modeset is performed here.
    /// Whether the runtime would accept a topology rebind.
    ///
    /// The owner waits for quiescence before applying a topology, but its wait
    /// used to consult only the native scanout, while the rebind demands this
    /// as well. The two definitions drifted, so the wait could pass and the
    /// rebind still fail. Both now read this.
    ///
    /// Distinct from the scanout's `output_topology_preparation_quiescent`,
    /// which asks whether a preparation may *begin*. That one is false
    /// throughout an installed candidate, which is precisely when this rebind
    /// runs, so the two cannot be collapsed. The wait ANDs them because it
    /// precedes both.
    pub fn topology_rebind_quiescent(&self) -> bool {
        !self.native_scanout_in_flight()
            && self.present_scheduler.in_flight_displayed_layer().is_none()
            && !self.present_scheduler.has_runnable_queued()
            && self.software_present_frames_waiting.is_empty()
            && self.software_present_frames_bound.is_empty()
            && self.software_presents_unframed.is_empty()
    }

    /// Every unmet clause, not just the first.
    ///
    /// Reporting only the first hid a second condition once already: a queued
    /// present and a waiting software frame produce different fixes, and the
    /// dispatch that would drain the queue is itself gated on no software frame
    /// waiting. One name could not distinguish them.
    /// Why the focused surface's present has not settled.
    ///
    /// Startup readiness waits for a present to be *stable*: displayed, with
    /// nothing newer submitted and no queued exporter frame. Every present
    /// being superseded instead is the difference between a session that comes
    /// up and one that never reports ready, and the two causes -- a present
    /// that never gets a turn, and one that is simply overtaken by fresher
    /// content -- need opposite fixes.
    pub fn present_supersession_report(&self) -> String {
        format!(
            "defers={} in_flight_displayed={} runnable_queued={}",
            self.present_output_busy_defers,
            u8::from(self.present_scheduler.in_flight_displayed_layer().is_some()),
            u8::from(self.present_scheduler.has_runnable_queued()),
        )
    }

    pub fn topology_rebind_quiescence_report(&self) -> String {
        let mut blockers = Vec::new();
        if self.native_scanout_in_flight() {
            blockers.push("native_scanout_in_flight");
        }
        if self.present_scheduler.in_flight_displayed_layer().is_some() {
            blockers.push("in_flight_displayed_layer");
        }
        if self.present_scheduler.has_runnable_queued() {
            blockers.push("runnable_queued_present");
        }
        if !self.software_present_frames_waiting.is_empty() {
            blockers.push("software_present_waiting");
        }
        if !self.software_present_frames_bound.is_empty() {
            blockers.push("software_present_bound");
        }
        if !self.software_presents_unframed.is_empty() {
            blockers.push("software_present_unframed");
        }
        if blockers.is_empty() {
            return "none".to_owned();
        }
        blockers.join("+")
    }

    pub fn rebind_applied_native_topology(
        &mut self,
        native_scanout: &mut LiveProductionNativeScanout,
        outputs: &[sophia_engine::HeadlessOutput],
        logical_viewports: &[(OutputId, Rect)],
    ) -> Result<(), Box<dyn std::error::Error>> {
        if !self.topology_rebind_quiescent() {
            return Err(
                "native topology runtime rebind requires quiescent presentation ownership".into(),
            );
        }
        let mut next = LiveProductionOutputRuntimeSet::adopt_native_topology(
            outputs,
            self.production.committed_surfaces(),
            native_scanout,
        )?;
        next.replace_logical_viewports(logical_viewports)?;
        let input_epoch = self
            .input_projections
            .iter()
            .map(|projection| projection.epoch)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or("presented input projection epoch exhausted")?;
        let input_projections = outputs
            .iter()
            .map(|output| LivePresentedInputProjection {
                output: output.id,
                epoch: input_epoch,
                layers: Vec::new(),
                chrome_targets: Vec::new(),
                chrome_occlusion: None,
                descriptor_targets: Vec::new(),
                descriptor_occlusion: None,
                descriptor_projection: None,
                tab_occlusions: Vec::new(),
                content: None,
            })
            .collect();
        let retained_outputs = outputs.iter().map(|output| output.id).collect();
        let _ = self.retain_shell_content_outputs(&retained_outputs)?;
        self.translations.settle();
        self.translation_deadlines.clear();
        native_scanout.set_translation_motion_active(false);
        self.content_layout_generation = self
            .content_layout_generation
            .checked_add(1)
            .ok_or("content layout generation exhausted")?;
        self.outputs = next;
        self.ordinary_repaints_pending.clear();
        self.input_projections = input_projections;
        Ok(())
    }

    pub fn drain_native_scanout(
        &mut self,
        native_scanout: &mut LiveProductionNativeScanout,
        timeout: Duration,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if !self.drain_native_scanout_until(native_scanout, timeout)? {
            return Err("persistent native scanout remained in flight during teardown".into());
        }
        Ok(())
    }

    /// Pure ownership check for terminal/revoked cleanup. Joining a renderer
    /// cannot dispose of singleton submitted/displayed/cleanup custody here.
    pub fn validate_native_retirement_disposition(&self) -> Result<(), &'static str> {
        if self.outputs.values().any(|output| {
            output.runtime.rendered_primary_plane_scanout_in_flight()
                || output.runtime.rendered_primary_plane_scanout_displayed()
                || output
                    .runtime
                    .rendered_primary_plane_scanout_cleanup_pending()
        }) {
            return Err("visual runtime retains unresolved native scanout custody");
        }
        Ok(())
    }

    fn drain_native_scanout_until(
        &mut self,
        native_scanout: &mut LiveProductionNativeScanout,
        timeout: Duration,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        for output in native_scanout.outputs() {
            native_scanout.cancel_prepared_output(output.id);
        }
        let deadline = Instant::now() + timeout;
        while (self.native_scanout_in_flight()
            || native_scanout.any_head_scanout_in_flight()
            || native_scanout.any_head_cleanup_pending())
            && Instant::now() < deadline
        {
            self.retire_native_scanout_for_drain(native_scanout)?;
            std::thread::sleep(Duration::from_millis(5));
        }
        if self.native_scanout_in_flight()
            || native_scanout.any_head_scanout_in_flight()
            || native_scanout.any_head_cleanup_pending()
        {
            return Ok(false);
        }
        let output_count = self.outputs.output_count();
        let production = &self.production;
        let outputs = &mut self.outputs;
        let mut adapter = crate::LiveProductionOutputRuntimeAdapter::new(
            output_count,
            |index, committed: &[CommittedSurfaceState]| -> Result<_, Box<dyn std::error::Error>> {
                // The adapter counts logical outputs; the scanout is addressed by
                // output identity.
                let output_id = outputs
                    .output_id(index)
                    .ok_or("production output index was not registered")?;
                let output = outputs
                    .values_mut()
                    .nth(index)
                    .ok_or("production output index was not registered")?;
                output
                    .runtime
                    .assembly_mut()
                    .replace_committed_surfaces(committed.to_vec());
                native_scanout.release_displayed_output(output_id, &mut output.runtime)
            },
        );
        let _ = production.run_outputs(&mut adapter)?;
        Ok(true)
    }

    pub(super) fn run_native_pending_output(
        &mut self,
        native_scanout: &mut LiveProductionNativeScanout,
        selected_output: OutputId,
    ) -> Result<crate::LiveBackendRuntimeTickReport, Box<dyn std::error::Error>> {
        let index = self
            .outputs
            .output_index(selected_output)
            .ok_or("frame service selected an unknown output")?;
        if !native_scanout.pending_frame(selected_output) {
            self.stage_software_present_frame(native_scanout, selected_output)?;
        }
        // Both views of the scene are taken after staging, from the same moment.
        // Reading the templates first let staging change the committed set
        // underneath them, and a template whose surface no longer matches its
        // committed state is rejected as an invalid surface -- which is what this
        // tick did the first time a mirror group ever reached it.
        let (layer_templates, committed) = self.scene_views();
        let output = self
            .outputs
            .values_mut()
            .nth(index)
            .ok_or("production output index was not registered")?;
        output
            .runtime
            .assembly_mut()
            .replace_committed_surfaces(committed);
        // Per-head state as well as the runtime's, or this guard is blind to a
        // mirror group and would tick an output whose heads are still in flight.
        let mirror = native_scanout.is_mirror_output(selected_output);
        if output.runtime.rendered_primary_plane_scanout_in_flight()
            || (!mirror && native_scanout.output_in_flight(selected_output))
            || output
                .runtime
                .rendered_primary_plane_scanout_cleanup_pending()
            || (!mirror && native_scanout.output_cleanup_pending(selected_output))
            || (mirror && native_scanout.primary_scanout_in_flight(selected_output))
            || (mirror && native_scanout.primary_cleanup_pending(selected_output))
            || !native_scanout.pending_frame(selected_output)
        {
            return Err("frame service selected an output that is not ready".into());
        }
        let report = native_scanout.run_tick(
            selected_output,
            &mut output.runtime,
            compositor_tick_input(&layer_templates, 0, Vec::new(), None),
        )?;
        use crate::LiveTrackedRenderedPrimaryPlaneScanoutSubmitStatus as Status;
        match report
            .rendered_primary_plane_scanout_submit
            .map(|submit| submit.status)
        {
            Some(Status::SubmittedWaitingForPageFlip) => {
                let submitted_content = native_scanout
                    .submitted_content(selected_output)
                    .ok_or("native submit did not retain its content identity")?;
                let expected_present = self
                    .present_scheduler
                    .unsubmitted_frame(selected_output)
                    .zip(self.present_scheduler.in_flight_transaction());
                self.settle_submission_ownership(
                    native_scanout,
                    selected_output,
                    submitted_content,
                    expected_present,
                )?;
                let submitted = native_scanout
                    .submitted_frame(selected_output)
                    .ok_or("native submit did not retain its frame identity")?;
                self.observe_software_present_frame_submitted(submitted)?;
            }
            Some(Status::ScanoutExportPending) | None => {}
            Some(Status::AlreadyInFlight | Status::CleanupPending) => {}
            Some(status) => {
                return Err(format!(
                    "Present output cohort failed while servicing output {}: submit_status={status:?}",
                    selected_output.raw()
                )
                .into());
            }
        }
        Ok(report)
    }

    pub fn retire_native_scanout(
        &mut self,
        native_scanout: &mut LiveProductionNativeScanout,
    ) -> Result<Option<LiveProductionRetiredPresent>, Box<dyn std::error::Error>> {
        let outputs = (0..self.outputs.output_count())
            .filter_map(|index| self.outputs.output_id(index))
            .collect::<Vec<_>>();
        let mut retired_present = None;
        for output in outputs {
            if let Some(retired) =
                self.retire_native_scanout_output_with_mode(native_scanout, output, false)?
            {
                retired_present = Some(retired);
            }
        }
        Ok(retired_present)
    }

    fn retire_native_scanout_for_drain(
        &mut self,
        native_scanout: &mut LiveProductionNativeScanout,
    ) -> Result<Option<LiveProductionRetiredPresent>, Box<dyn std::error::Error>> {
        let outputs = (0..self.outputs.output_count())
            .filter_map(|index| self.outputs.output_id(index))
            .collect::<Vec<_>>();
        let mut retired_present = None;
        for output in outputs {
            if let Some(retired) =
                self.retire_native_scanout_output_with_mode(native_scanout, output, true)?
            {
                retired_present = Some(retired);
            }
        }
        Ok(retired_present)
    }

    pub(super) fn retire_native_scanout_output(
        &mut self,
        native_scanout: &mut LiveProductionNativeScanout,
        selected_output: OutputId,
    ) -> Result<Option<LiveProductionRetiredPresent>, Box<dyn std::error::Error>> {
        self.retire_native_scanout_output_with_mode(native_scanout, selected_output, false)
    }

    fn retire_native_scanout_output_with_mode(
        &mut self,
        native_scanout: &mut LiveProductionNativeScanout,
        selected_output: OutputId,
        draining: bool,
    ) -> Result<Option<LiveProductionRetiredPresent>, Box<dyn std::error::Error>> {
        let index = self
            .outputs
            .output_index(selected_output)
            .ok_or("frame service selected an unknown retirement output")?;
        // Authority and repaint cycles may submit a staged frame between
        // frame-service passes. The scanout owner retains that exact identity
        // until retirement, so observe it before consuming the callback.
        if let Some(submitted) = native_scanout.submitted_frame(selected_output) {
            self.observe_software_present_frame_submitted(submitted)?;
        }
        let committed = self.production.committed_surfaces().to_vec();
        let output = self
            .outputs
            .values_mut()
            .nth(index)
            .ok_or("production output index was not registered")?;
        output
            .runtime
            .assembly_mut()
            .replace_committed_surfaces(committed);
        if draining {
            native_scanout.retire_ready_for_drain(selected_output, &mut output.runtime)?;
        } else {
            native_scanout.retire_ready_and_retry_cleanup(selected_output, &mut output.runtime)?;
        }
        if let Some(retirement) = native_scanout.take_presentation_feedback(selected_output) {
            // Any retirement on this output means a successor flip has taken
            // the plane, so a client buffer displayed directly before it is no
            // longer being scanned and may be idled. Done here rather than in
            // the Present arm below because a composed successor is a
            // successor too -- an overlay opening returns the output to
            // composition, and the direct frame it replaced is owed its
            // release just the same.
            // See `PresentFlipOwnership.tla`, `SuccessorComposedRetires`.
            self.idle_superseded_direct_present(selected_output, Some(native_scanout))?;
            match reduce_live_production_native_retirement_owner(
                retirement.frame,
                retirement.content,
                self.present_scheduler.submitted_frame(selected_output),
                self.present_scheduler
                    .owns_frame(selected_output, retirement.frame),
            ) {
                LiveProductionNativeRetirementOwner::IndependentFrame => {
                    let settlement = self.settle_software_present_frame(retirement)?;
                    if !matches!(
                        settlement,
                        software_present::LiveProductionSoftwarePresentSettlement::Waiting
                    ) {
                        self.publish_presented_input_layers(native_scanout);
                    }
                    return Ok(None);
                }
                LiveProductionNativeRetirementOwner::SubmittedDmaPresent => {
                    let retired =
                        self.finalize_gpu_page_flip(native_scanout, selected_output, retirement)?;
                    if retired.is_some() {
                        self.publish_presented_input_layers(native_scanout);
                    }
                    return Ok(retired);
                }
                LiveProductionNativeRetirementOwner::SupersededDmaPresent => {
                    self.settle_superseded_retirement(selected_output, retirement.frame);
                    self.publish_presented_input_layers(native_scanout);
                    return Ok(None);
                }
                LiveProductionNativeRetirementOwner::InvalidDmaOwnership => {
                    return Err(self
                        .ownership_mismatch(selected_output, retirement)
                        .to_string()
                        .into());
                }
            }
        }
        self.publish_presented_input_layers(native_scanout);
        Ok(None)
    }

    pub fn finalize_gpu_page_flip(
        &mut self,
        native_scanout: &mut LiveProductionNativeScanout,
        output: OutputId,
        retirement: LiveProductionNativeFrameRetirement,
    ) -> Result<Option<LiveProductionRetiredPresent>, Box<dyn std::error::Error>> {
        if reduce_live_production_native_retirement_owner(
            retirement.frame,
            retirement.content,
            self.present_scheduler.submitted_frame(output),
            self.present_scheduler.owns_frame(output, retirement.frame),
        ) != LiveProductionNativeRetirementOwner::SubmittedDmaPresent
        {
            return Err("GPU retirement does not own the selected output frame".into());
        }
        if !matches!(
            retirement.content,
            LiveProductionScanoutContent::MixedPresent { transaction, .. }
                if Some(transaction) == self.present_scheduler.in_flight_transaction()
        ) {
            return Err(self
                .ownership_mismatch(output, retirement)
                .to_string()
                .into());
        }
        let terminal =
            self.present_scheduler
                .mark_output_retired(LiveProductionPageFlipRetirement {
                    output,
                    ust: retirement.ust,
                    msc: retirement.msc,
                })?;
        let Some(sophia_engine::TransactionPresentationTerminal::Presented { .. }) = terminal
        else {
            return Ok(None);
        };
        let (surface, layer) = self
            .present_scheduler
            .in_flight_displayed_layer()
            .ok_or("joined native retirement lost its renderer image owner")?;
        let image = layer.image_id;
        let transaction = self
            .present_scheduler
            .in_flight_transaction()
            .ok_or("joined native retirement lost its transaction owner")?;
        // The page flip is the commit point for the compositor copy. Promote
        // its staged image before releasing the client source or emitting any
        // protocol feedback.
        //
        // A direct frame has no such image. Nothing was composed, so nothing
        // was staged: the buffer on the plane is the client's own, and the
        // renderer never saw it. Demanding a snapshot here failed the first
        // frame that ever reached a plane directly -- after it had already
        // been displayed, which made a working flip look like a lost one.
        if !retirement.direct && native_scanout.promote_renderer_image(image)? == 0 {
            return Err(format!(
                "retired Present lost its staged renderer snapshot: transaction={} surface={} image={} output={} frame={}",
                transaction.raw(), surface.index(), image.raw(), output.raw(), retirement.frame.raw(),
            ).into());
        }
        let submitted = self
            .present_scheduler
            .take_submitted()
            .ok_or("joined native retirement lost its submitted DMA Present")?;
        let layout_identity = layout_witness::SubmittedLayoutIdentity::from_submitted(&submitted);
        let clock = submitted
            .presentation_clock()
            .ok_or("joined native retirement retained no physical presentation clock")?;
        let ust = clock.ust;
        let msc = clock.msc;
        let outputs = submitted.frames().map(|(output, _)| output).collect();
        let direct = retirement.direct;
        let (production, presentation_feedback) =
            (&mut self.production, &mut self.presentation_feedback);
        let mut completion = production
            .settle_prepared_retirement(submitted.prepared, |commit| match commit.outcome {
                // A direct frame completes without idling: the buffer the
                // client handed over is the buffer the screen is scanning, and
                // releasing it here would let the client draw into displayed
                // pixels. Its successor idles it, above, on the next flip.
                TransactionOutcome::Committed if direct => presentation_feedback
                    .complete_flip_without_idle(submitted.transaction, ust, msc),
                TransactionOutcome::Committed => {
                    presentation_feedback.complete_copy(submitted.transaction, ust, msc)
                }
                TransactionOutcome::RejectedStaleSurface
                | TransactionOutcome::RejectedInvalidSurface
                | TransactionOutcome::TimedOut => {
                    presentation_feedback.reject_skip(submitted.transaction, ust, msc)
                }
            })
            .map_err(|error| format!("page flip protocol settlement failed: {error:?}"))?;
        let layout_witness = layout_identity.and_then(|identity| {
            identity.settle_feedback(retirement, &completion.commit, &mut completion.evidence)
        });
        self.outputs
            .project_committed(&completion.committed_surfaces);
        self.route_present_feedback(completion.evidence);
        if completion.commit.outcome != TransactionOutcome::Committed {
            self.present_rejections = self.present_rejections.saturating_add(1);
        }
        if direct && completion.commit.outcome == TransactionOutcome::Committed {
            self.displayed_direct_presents
                .insert(output, submitted.transaction);
        }
        let deferred_groups = self.finish_surface_content_owner(submitted.candidate)?;
        if deferred_groups != 0 {
            tracing::debug!(
                transaction = submitted.transaction.raw(),
                surface = submitted.surface.index(),
                deferred_groups,
                "retired Present released its ordered surface authority backlog"
            );
        }
        if completion.commit.outcome != TransactionOutcome::Committed {
            // Nothing to evict for a direct frame, for the same reason nothing
            // was promoted; eviction of an image no exporter staged is a
            // no-op, so this is stated rather than branched.
            native_scanout.evict_renderer_image(submitted.displayed_layer.image_id)?;
            tracing::warn!(
                transaction = completion.commit.transaction.raw(),
                outcome = ?completion.commit.outcome,
                "settled retired Present without applying its stale Engine candidate"
            );
            return Ok(None);
        }
        let source_size = submitted.displayed_layer.size;
        let target = submitted.displayed_layer.placement.target;
        let clip = submitted.displayed_layer.placement.clip;
        let replaced = replace_displayed_surface(
            &mut self.displayed_surfaces,
            submitted.surface,
            submitted.displayed_layer,
        );
        if let Some(replaced) = replaced {
            native_scanout.evict_renderer_image(replaced.layer.image_id)?;
        }
        Ok(Some(LiveProductionRetiredPresent {
            candidate: submitted.candidate,
            transaction: submitted.transaction,
            surface: submitted.surface,
            outputs,
            source_size,
            target,
            clip,
            ust_usec: ust,
            msc,
            layout_witness,
        }))
    }

    pub fn native_scanout_in_flight(&self) -> bool {
        self.outputs.native_scanout_in_flight()
    }

    pub fn native_cleanup_pending(&self) -> bool {
        self.outputs.native_cleanup_pending()
    }

    pub fn native_diagnostic(&self) -> String {
        self.outputs.diagnostic()
    }
}
