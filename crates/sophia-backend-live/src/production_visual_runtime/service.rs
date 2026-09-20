use super::*;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LiveProductionCursorPresentation {
    HardwarePlane,
    Software(Option<Point>),
}

impl LiveProductionCursorPresentation {
    pub const fn composition_position(self) -> Option<Point> {
        match self {
            Self::HardwarePlane => None,
            Self::Software(position) => position,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LiveProductionVisualDiagnostics {
    pub present_queued: bool,
    pub present_scheduling_blocked: bool,
    pub live_sources: usize,
    pub live_fences: usize,
    pub live_presentations: usize,
    pub max_live_sources: usize,
    pub max_live_fences: usize,
    pub max_live_presentations: usize,
    pub pending_supersessions: usize,
    pub surface_content_supersessions: usize,
    pub scheduler_supersessions: usize,
    pub max_surface_content_deferred: usize,
    pub max_latest_deferred_per_surface: usize,
    pub max_pending_queued: usize,
    pub max_total_queued: usize,
    /// Presents held to the head's refresh because no head could carry them.
    pub paced_skips: usize,
    pub max_frame_tick_parked: usize,
    pub frame_tick_overflows: usize,
    pub software_present_frames_waiting: usize,
    pub software_present_frames_submitted: usize,
    pub software_present_retirements_pending: usize,
    pub acquire_waits: usize,
    pub controlled_rejections: usize,
    pub present_rejections: usize,
    pub native_suspend_present_rejections: usize,
    pub topology_escalation_present_rejections: usize,
    pub other_present_rejections: usize,
    pub shutdown_present_rejections: usize,
}

impl LiveProductionVisualRuntime {
    pub fn diagnostics(&self) -> LiveProductionVisualDiagnostics {
        LiveProductionVisualDiagnostics {
            present_queued: self.present_scheduler.has_runnable_queued(),
            present_scheduling_blocked: self.present_scheduler.has_layout_deferred()
                && !self.present_scheduler.has_eligible(),
            live_sources: self.presentation_feedback.resources().source_count(),
            live_fences: self.presentation_feedback.resources().fence_count(),
            live_presentations: self.presentation_feedback.resources().presentation_count(),
            max_live_sources: self.presentation_feedback.resources().max_source_count(),
            max_live_fences: self.presentation_feedback.resources().max_fence_count(),
            max_live_presentations: self
                .presentation_feedback
                .resources()
                .max_presentation_count(),
            pending_supersessions: self
                .surface_content_stream
                .supersessions()
                .saturating_add(self.present_scheduler.pending_supersessions()),
            surface_content_supersessions: self.surface_content_stream.supersessions(),
            scheduler_supersessions: self.present_scheduler.pending_supersessions(),
            max_surface_content_deferred: self.surface_content_stream.max_deferred_len(),
            max_latest_deferred_per_surface: self
                .surface_content_stream
                .max_latest_deferred_per_surface(),
            max_pending_queued: self.present_scheduler.max_pending_queued(),
            max_total_queued: self.present_scheduler.max_total_queued(),
            paced_skips: self.present_scheduler.paced_skips(),
            max_frame_tick_parked: self.present_scheduler.max_frame_tick_parked(),
            frame_tick_overflows: self.present_scheduler.frame_tick_overflows(),
            software_present_frames_waiting: self
                .software_presents_unframed
                .len()
                .saturating_add(self.software_present_frames_waiting.len())
                .saturating_add(
                    self.software_present_frames_bound
                        .values()
                        .filter(|binding| {
                            binding.phase == LiveProductionSoftwarePresentFramePhase::Pending
                        })
                        .count(),
                ),
            software_present_frames_submitted: self
                .software_present_frames_bound
                .values()
                .filter(|binding| {
                    binding.phase == LiveProductionSoftwarePresentFramePhase::Submitted
                })
                .count(),
            software_present_retirements_pending: self.retired_software_presents.len(),
            acquire_waits: self.present_scheduler.acquire_waits(),
            controlled_rejections: self.present_scheduler.controlled_rejections(),
            present_rejections: self.present_rejections,
            native_suspend_present_rejections: self.native_suspend_present_rejections,
            topology_escalation_present_rejections: self.topology_escalation_present_rejections,
            other_present_rejections: self.present_rejections.saturating_sub(
                self.surface_content_stream
                    .supersessions()
                    .saturating_add(self.present_scheduler.pending_supersessions())
                    .saturating_add(self.present_scheduler.controlled_rejections())
                    .saturating_add(self.native_suspend_present_rejections)
                    .saturating_add(self.topology_escalation_present_rejections)
                    .saturating_add(self.shutdown_present_rejections),
            ),
            shutdown_present_rejections: self.shutdown_present_rejections,
        }
    }

    pub fn replace_output_projection(
        &mut self,
        index: usize,
        committed: Vec<CommittedSurfaceState>,
    ) -> bool {
        self.outputs.replace_output_projection(index, committed)
    }

    pub fn output_count(&self) -> usize {
        self.outputs.output_count()
    }

    pub fn logical_viewports(&self) -> Vec<(OutputId, Rect)> {
        self.outputs.logical_viewports().collect()
    }

    pub fn output_committed(&self, index: usize) -> Option<&[CommittedSurfaceState]> {
        self.outputs.output_committed(index)
    }
}

#[derive(Debug)]
pub struct LiveProductionRetiredPresent {
    pub candidate: SurfaceTransactionKey,
    pub transaction: TransactionId,
    pub surface: SurfaceId,
    pub outputs: Vec<OutputId>,
    pub source_size: Size,
    pub target: Rect,
    pub clip: Option<Rect>,
    pub ust_usec: u64,
    pub msc: u64,
    /// Exact single-output copied evidence; frontend preferences and opt-in are separate gates.
    pub layout_witness: Option<LiveProductionRetiredLayoutWitness>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LiveProductionRetiredSoftwarePresent {
    pub candidate: SurfaceTransactionKey,
    pub source_size: Size,
    pub frame: LiveProductionNativeFrameId,
    pub native_submission: u64,
    pub ust_usec: u64,
    pub msc: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveProductionSoftwarePresentFramePhase {
    Pending,
    Submitted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveProductionSoftwarePresentFrameObservation {
    NativeSubmitted(LiveProductionNativeFrameId),
    NativeRetired(LiveProductionNativeFrameId),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveProductionSoftwarePresentFrameTransition {
    Unrelated,
    Submitted,
    AlreadySubmitted,
    Retired,
    InvalidRetirement,
}

pub fn reduce_software_present_frame_observation(
    owned_frame: LiveProductionNativeFrameId,
    phase: LiveProductionSoftwarePresentFramePhase,
    observation: LiveProductionSoftwarePresentFrameObservation,
) -> LiveProductionSoftwarePresentFrameTransition {
    match observation {
        LiveProductionSoftwarePresentFrameObservation::NativeSubmitted(frame)
            if frame == owned_frame
                && matches!(phase, LiveProductionSoftwarePresentFramePhase::Pending) =>
        {
            LiveProductionSoftwarePresentFrameTransition::Submitted
        }
        LiveProductionSoftwarePresentFrameObservation::NativeSubmitted(frame)
            if frame == owned_frame
                && matches!(phase, LiveProductionSoftwarePresentFramePhase::Submitted) =>
        {
            LiveProductionSoftwarePresentFrameTransition::AlreadySubmitted
        }
        LiveProductionSoftwarePresentFrameObservation::NativeRetired(frame)
            if frame == owned_frame
                && matches!(phase, LiveProductionSoftwarePresentFramePhase::Submitted) =>
        {
            LiveProductionSoftwarePresentFrameTransition::Retired
        }
        LiveProductionSoftwarePresentFrameObservation::NativeRetired(frame)
            if frame == owned_frame =>
        {
            LiveProductionSoftwarePresentFrameTransition::InvalidRetirement
        }
        _ => LiveProductionSoftwarePresentFrameTransition::Unrelated,
    }
}

/// A retired Present whose Engine candidate was not applied.
///
/// The flip showed the candidate's pixels, so the screen and the committed set
/// disagree until the surface's next Present lands. The generations say why: a
/// `current_generation` above `baseline_generation` means an authority intake
/// landed between prepare and retire.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LiveProductionDiscardedPresent {
    pub transaction: TransactionId,
    pub surface: SurfaceId,
    pub outcome: TransactionOutcome,
    pub baseline_generation: u64,
    pub current_generation: u64,
}

#[derive(Debug)]
pub struct LiveProductionNativeServiceReport {
    pub ticks: Vec<LiveBackendRuntimeTickReport>,
    pub retired_present: Option<LiveProductionRetiredPresent>,
    pub retired_software_presents: Vec<LiveProductionRetiredSoftwarePresent>,
    pub discarded_presents: Vec<LiveProductionDiscardedPresent>,
    pub effects: Vec<OutputFrameServiceEffect>,
}

pub const fn reduce_output_native_frame_phase(
    in_flight: bool,
    cleanup_pending: bool,
) -> OutputNativeFramePhase {
    if cleanup_pending {
        OutputNativeFramePhase::CleanupPending
    } else if in_flight {
        OutputNativeFramePhase::InFlight
    } else {
        OutputNativeFramePhase::Idle
    }
}

impl LiveProductionVisualRuntime {
    pub fn native_output_service_request(
        &self,
        native_scanout: &LiveProductionNativeScanout,
    ) -> Result<OutputFrameServiceRequest, Box<dyn std::error::Error>> {
        let primary = self
            .outputs
            .primary_output()
            .ok_or("persistent backend runtime has no primary output")?;
        let software_frame_waiting = self.software_present_frames_waiting.front();
        let presentation_queued = software_frame_waiting.is_none()
            && self.diagnostics().present_queued
            && !self.diagnostics().present_scheduling_blocked;
        let outputs = (0..self.output_count())
            .map(|index| {
                let output = self
                    .outputs
                    .output_id(index)
                    .ok_or("production output index was not registered")?;
                // Both sources, because a mirror group's work lives in per-head
                // slots and an ungrouped output's lives in the runtime's single
                // one. Reading only the runtime reported a grouped output as Idle
                // forever, so the frame service never polled it for retirement and
                // Present completions were never routed back to the client.
                let pending_frame = native_scanout.pending_frame(output);
                let logical_work_waiting = pending_frame
                    || (output == primary
                        && (presentation_queued || software_frame_waiting.is_some()));
                let primary_lane = native_scanout.is_mirror_output(output) && logical_work_waiting;
                let in_flight = self
                    .outputs
                    .output_native_scanout_in_flight(index)
                    .ok_or("production output in-flight state was not registered")?
                    || if primary_lane {
                        native_scanout.primary_scanout_in_flight(output)
                    } else {
                        native_scanout.output_in_flight(output)
                    };
                let cleanup_pending = self
                    .outputs
                    .output_native_cleanup_pending(index)
                    .ok_or("production output cleanup state was not registered")?
                    || if primary_lane {
                        native_scanout.primary_cleanup_pending(output)
                    } else {
                        native_scanout.output_cleanup_pending(output)
                    };
                Ok(OutputFrameServiceObservation {
                    output,
                    primary: output == primary,
                    native_phase: reduce_output_native_frame_phase(in_flight, cleanup_pending),
                    // Native work owed to this output only. A waiting software
                    // present is reported once on the request instead, so the
                    // reducer can tell "this output owes a frame" apart from
                    // "something global is pending".
                    pending_frame,
                })
            })
            .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?;
        Ok(OutputFrameServiceRequest {
            outputs,
            presentation_queued,
            software_frame_waiting: software_frame_waiting.is_some(),
        })
    }

    pub fn service_native(
        &mut self,
        native_scanout: &mut LiveProductionNativeScanout,
        scene: &LiveProductionCpuScene,
    ) -> Result<LiveProductionNativeServiceReport, Box<dyn std::error::Error>> {
        // One readiness pass per card, before any output can retire, submit a
        // successor, or declare its outstanding request stalled.
        native_scanout.pump_native_completions()?;
        // Optional evidence owns resources even when no frame is pending.
        native_scanout.service_layout_probe_cleanup();
        if self.retained_projection_pending {
            self.queue_retained_projection(scene, native_scanout)?;
        }
        self.service_ordinary_repaints(scene, native_scanout)?;
        self.service_translation_frames(native_scanout, scene)?;
        let initial = self.native_output_service_request(native_scanout)?;
        let mut reducer = OutputFrameServiceReducer::begin(&initial)
            .map_err(|error| format!("invalid output frame service state: {error:?}"))?;
        let mut ticks = Vec::new();
        let mut retired_present = None;
        let mut effects = Vec::new();
        loop {
            let observation = self.native_output_service_request(native_scanout)?;
            let Some(effect) = reducer
                .next_effect(&observation)
                .map_err(|error| format!("output frame service reduction failed: {error:?}"))?
            else {
                break;
            };
            effects.push(effect);
            match effect {
                OutputFrameServiceEffect::PollRetirement { output } => {
                    if let Some(retired) =
                        self.retire_native_scanout_output(native_scanout, output)?
                    {
                        retired_present = Some(retired);
                    }
                }
                OutputFrameServiceEffect::SubmitQueuedPresentation { output } => {
                    if output != reducer.primary() {
                        return Err("queued presentation targeted a non-primary output".into());
                    }
                    ticks.push(self.drive_gpu_presentation(scene, Some(native_scanout))?);
                }
                OutputFrameServiceEffect::SubmitPendingFrame { output } => {
                    ticks.push(self.run_native_pending_output(native_scanout, output)?);
                }
            }
        }
        // Close the arrival race at the hard-stall boundary without adding a
        // second DRM read to ordinary service cycles. A head that crossed the
        // deadline gets one final card-scoped collect-and-retire pass; only an
        // outstanding request still lacking both proofs may terminate.
        if native_scanout.page_flip_hard_stall().is_some() {
            native_scanout.pump_native_completions()?;
            self.service_translation_frames(native_scanout, scene)?;
            if let Some(retired) = self.retire_native_scanout(native_scanout)? {
                retired_present = Some(retired);
            }
        }
        let mut retired_software_presents = Vec::new();
        self.drain_retired_software_presents_into(&mut retired_software_presents)?;
        native_scanout.ensure_page_flip_progress()?;
        native_scanout.service_idle_atomic_cursors()?;
        Ok(LiveProductionNativeServiceReport {
            ticks,
            retired_present,
            retired_software_presents,
            discarded_presents: std::mem::take(&mut self.discarded_presents),
            effects,
        })
    }
}
