use super::*;

pub(super) enum LiveProductionSoftwarePresentSettlement {
    NotOwned,
    Waiting,
    Settled,
}

pub(super) struct LiveProductionSoftwarePresentFrame {
    pub source_set: compositor_graphics::LiveProductionRetainedCompositionSourceSet,
    pub submissions: Vec<LiveProductionSoftwarePresentSubmission>,
}

pub(super) struct LiveProductionSoftwarePresentBinding {
    pub source_set: compositor_graphics::LiveProductionRetainedCompositionSourceSet,
    pub frames: BTreeMap<OutputId, LiveProductionNativeFrameId>,
    pub clock_output: OutputId,
    pub output_cohort: sophia_engine::TransactionPresentationCohort,
    pub retirements: BTreeMap<OutputId, LiveProductionNativeFrameRetirement>,
    pub submissions: Vec<LiveProductionSoftwarePresentSubmission>,
    pub phase: LiveProductionSoftwarePresentFramePhase,
}

impl LiveProductionVisualRuntime {
    pub(super) fn frame_unframed_software_presents(
        &mut self,
        scene: &mut LiveProductionCpuScene,
        output_descriptors: &[HeadlessOutput],
    ) -> Result<(), Box<dyn std::error::Error>> {
        let submissions = self
            .software_presents_unframed
            .drain(..)
            .flatten()
            .collect::<Vec<_>>();
        // Every software Present committed in this cycle owns the same
        // immutable composition, so keep the set attached to that frame.
        self.queue_software_present_frame(scene, output_descriptors, submissions)
    }

    pub(super) fn settle_unframed_software_presents_without_native(
        &mut self,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let submissions = self
            .software_presents_unframed
            .drain(..)
            .flatten()
            .collect::<Vec<_>>();
        for submission in submissions {
            self.presentation_feedback
                .resources_mut()
                .mark_submitted(submission.transaction)?;
            let outcome = self
                .presentation_feedback
                .complete_copy(submission.transaction, 0, 0)?;
            self.route_present_feedback(outcome);
            if self.retired_software_presents.len() == PRESENT_FEEDBACK_CAPACITY {
                self.retired_software_presents_overflowed = true;
            } else {
                self.retired_software_presents
                    .push_back(LiveProductionRetiredSoftwarePresent {
                        candidate: submission.candidate,
                        source_size: submission.source_size,
                        frame: LiveProductionNativeFrameId::from_raw(0),
                        native_submission: 0,
                        ust_usec: 0,
                        msc: 0,
                    });
            }
            self.finish_surface_content_owner(submission.candidate)?;
        }
        Ok(())
    }

    pub(super) fn queue_software_present_frame(
        &mut self,
        scene: &mut LiveProductionCpuScene,
        _output_descriptors: &[HeadlessOutput],
        submissions: Vec<LiveProductionSoftwarePresentSubmission>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if submissions.is_empty() {
            return Ok(());
        }
        if self
            .software_present_frames_waiting
            .len()
            .saturating_add(self.software_present_frames_bound.len())
            >= PRESENT_FEEDBACK_CAPACITY
        {
            return Err("production software Present frame queue overflowed".into());
        }
        // The software Present path shares the scheduler, so a direct GPU
        // submission can be in flight while a CPU frame queues; it has no
        // native handle here, so only the *displayed* direct frames resolve.
        // The in-flight one keeps its prior behaviour on this path.
        let source_set = self.retained_composition_source_set(scene, None)?;
        self.software_present_frames_waiting
            .push_back(LiveProductionSoftwarePresentFrame {
                source_set,
                submissions,
            });
        Ok(())
    }

    pub(super) fn stage_software_present_frame<T: NativeCompositionTarget>(
        &mut self,
        native_scanout: &mut T,
        output: OutputId,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        if self.native_publication_blocked() {
            return Ok(false);
        }
        if native_scanout.head_targets(output).is_empty() {
            return Err("software Present targeted an unknown native output".into());
        }
        let required_outputs = self
            .outputs
            .logical_viewports()
            .map(|(output, _)| output)
            .collect::<Vec<_>>();
        if !native_scanout.software_outputs_ready(&required_outputs.iter().copied().collect()) {
            return Ok(false);
        }
        let Some(waiting) = self.software_present_frames_waiting.front() else {
            return Ok(false);
        };
        let batches = self.retained_output_head_composition_frames_from_sources(
            native_scanout,
            &waiting.source_set,
        )?;
        let cohort_transaction = waiting
            .submissions
            .first()
            .map(|submission| submission.transaction)
            .ok_or("software Present cohort has no submission")?;
        let output_cohort = sophia_engine::TransactionPresentationCohort::new(
            cohort_transaction,
            batches.iter().map(|(output, _)| *output),
        )
        .ok_or("software Present cohort has invalid output coverage")?;
        if !output_cohort.is_required(output) {
            return Err("software Present cohort omitted its selected clock output".into());
        }
        let frames = native_scanout.queue_software_batch(batches)?;
        let root = frames
            .values()
            .next()
            .copied()
            .ok_or("software Present cohort produced no native frame")?;
        if frames.values().any(|frame| {
            self.software_present_frame_owners.contains_key(frame)
                || self.software_present_frames_bound.contains_key(frame)
        }) {
            return Err("native frame ID was reused for software Present".into());
        }
        // Keep the frame in the waiting queue through every fallible staging
        // operation. If native queueing fails, shutdown must still find its
        // submissions and release their SurfaceContentStream ownership.
        let mut waiting = self
            .software_present_frames_waiting
            .pop_front()
            .expect("software Present frame front retained through staging");
        waiting.source_set._image_reads = self.image_reads_for_sources(&waiting.source_set.sources);
        for frame in frames.values() {
            self.software_present_frame_owners.insert(*frame, root);
        }
        let replaced = self.software_present_frames_bound.insert(
            root,
            LiveProductionSoftwarePresentBinding {
                source_set: waiting.source_set,
                frames,
                clock_output: output,
                output_cohort,
                retirements: BTreeMap::new(),
                submissions: waiting.submissions,
                phase: LiveProductionSoftwarePresentFramePhase::Pending,
            },
        );
        debug_assert!(replaced.is_none(), "software Present owner checked above");
        tracing::debug!(
            outputs = required_outputs.len(),
            frame = root.raw(),
            "bound software Present work to an immutable native output cohort"
        );
        Ok(true)
    }

    pub(super) fn observe_software_present_frame_submitted(
        &mut self,
        frame: LiveProductionNativeFrameId,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let Some(root) = self.software_present_frame_owners.get(&frame).copied() else {
            return Ok(());
        };
        let binding = self
            .software_present_frames_bound
            .get_mut(&root)
            .ok_or("software Present frame lost its cohort owner")?;
        let output = binding
            .frames
            .iter()
            .find_map(|(output, owned)| (*owned == frame).then_some(*output))
            .ok_or("software Present cohort does not own submitted frame")?;
        use sophia_engine::TransactionPresentationTransition as Transition;
        match binding.output_cohort.mark_submitted(output) {
            Transition::PhaseReady => {
                for submission in &binding.submissions {
                    self.presentation_feedback
                        .resources_mut()
                        .mark_submitted(submission.transaction)?;
                }
                binding.phase = LiveProductionSoftwarePresentFramePhase::Submitted;
            }
            Transition::Accepted | Transition::Duplicate => {}
            _ => return Err("software Present cohort rejected output submission".into()),
        }
        Ok(())
    }

    pub(super) fn settle_software_present_frame(
        &mut self,
        retirement: LiveProductionNativeFrameRetirement,
    ) -> Result<LiveProductionSoftwarePresentSettlement, Box<dyn std::error::Error>> {
        let Some(root) = self
            .software_present_frame_owners
            .get(&retirement.frame)
            .copied()
        else {
            return Ok(LiveProductionSoftwarePresentSettlement::NotOwned);
        };
        let terminal = {
            let binding = self
                .software_present_frames_bound
                .get_mut(&root)
                .ok_or("software Present retirement lost its cohort owner")?;
            let output = binding
                .frames
                .iter()
                .find_map(|(output, frame)| (*frame == retirement.frame).then_some(*output))
                .ok_or("software Present cohort does not own retired frame")?;
            let ust = retirement.ust;
            binding.retirements.insert(output, retirement);
            binding.output_cohort.mark_retired(output, ust)
        };
        use sophia_engine::TransactionPresentationTransition as Transition;
        if !matches!(terminal, Transition::PhaseReady) {
            return match terminal {
                Transition::Accepted | Transition::Duplicate => {
                    Ok(LiveProductionSoftwarePresentSettlement::Waiting)
                }
                _ => Err("software Present cohort rejected output retirement".into()),
            };
        }
        let binding = self
            .software_present_frames_bound
            .remove(&root)
            .ok_or("software Present terminal cohort disappeared")?;
        for frame in binding.frames.values() {
            self.software_present_frame_owners.remove(frame);
        }
        let terminal = binding
            .output_cohort
            .terminal()
            .ok_or("software Present cohort reached no terminal state")?;
        let sophia_engine::TransactionPresentationTerminal::Presented { .. } = terminal else {
            return Err("software Present output cohort failed".into());
        };
        let evidence = binding
            .retirements
            .get(&binding.clock_output)
            .ok_or("software Present cohort retained no retirement evidence")?;
        let clocks = crate::LiveNativeRetirementClocks::from_evidence(
            binding
                .retirements
                .values()
                .flat_map(|retirement| retirement.clocks.evidence().iter().copied()),
        );
        for submission in binding.submissions {
            let mut outcome = self.presentation_feedback.complete_copy(
                submission.transaction,
                evidence.ust,
                evidence.msc,
            )?;
            outcome.clocks = clocks.clone();
            self.route_present_feedback(outcome);
            if self.retired_software_presents.len() == PRESENT_FEEDBACK_CAPACITY {
                self.retired_software_presents_overflowed = true;
            } else {
                self.retired_software_presents
                    .push_back(LiveProductionRetiredSoftwarePresent {
                        candidate: submission.candidate,
                        source_size: submission.source_size,
                        frame: evidence.frame,
                        native_submission: evidence.submission,
                        ust_usec: evidence.ust,
                        msc: evidence.msc,
                    });
            }
            self.finish_surface_content_owner(submission.candidate)?;
        }
        Ok(LiveProductionSoftwarePresentSettlement::Settled)
    }

    pub(super) fn reject_software_present_frames(&mut self) -> usize {
        let submissions = self
            .software_presents_unframed
            .drain(..)
            .flatten()
            .chain(
                self.software_present_frames_waiting
                    .drain(..)
                    .flat_map(|frame| frame.submissions),
            )
            .chain(
                std::mem::take(&mut self.software_present_frames_bound)
                    .into_values()
                    .flat_map(|binding| binding.submissions),
            )
            .collect::<Vec<_>>();
        self.software_present_frame_owners.clear();
        let mut rejected = 0usize;
        for submission in submissions {
            if let Ok(outcome) = self
                .presentation_feedback
                .reject_skip_at_last_display(submission.transaction)
            {
                self.route_present_feedback(outcome);
                rejected = rejected.saturating_add(1);
                self.present_rejections = self.present_rejections.saturating_add(1);
            }
            if let Err(error) = self.finish_surface_content_owner(submission.candidate) {
                tracing::error!(
                    transaction = submission.transaction.raw(),
                    %error,
                    "failed to release rejected software Present content owner"
                );
            }
        }
        rejected
    }
}
