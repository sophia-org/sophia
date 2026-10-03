use sophia_engine::{
    EngineHeadRegistry, OutputPresentationFeedback, OutputPresentationRegistry,
    OutputPresentationRetire, OutputPresentationSchedule, ProductionOutputRuntimeAdapter,
    ProductionPresentationAdapter, ProductionRetirement,
};
#[cfg(all(feature = "libdrm-events", feature = "gbm-probe"))]
use sophia_protocol::TransactionId;
use sophia_protocol::{CommittedSurfaceState, OutputId};
use std::collections::{BTreeMap, VecDeque};

pub struct LiveProductionOutputRuntimeAdapter<Run> {
    output_count: usize,
    run: Run,
}

impl<Run> LiveProductionOutputRuntimeAdapter<Run> {
    pub const fn new(output_count: usize, run: Run) -> Self {
        Self { output_count, run }
    }
}

impl<Run, Report, Error> ProductionOutputRuntimeAdapter for LiveProductionOutputRuntimeAdapter<Run>
where
    Run: FnMut(usize, &[CommittedSurfaceState]) -> Result<Report, Error>,
{
    type Report = Report;
    type Error = Error;

    fn output_count(&self) -> usize {
        self.output_count
    }

    fn run_output(
        &mut self,
        output_index: usize,
        committed: &[CommittedSurfaceState],
    ) -> Result<Self::Report, Self::Error> {
        (self.run)(output_index, committed)
    }
}

pub struct LiveProductionPresentationAdapter<Compose, Submit, Retire, Feedback> {
    compose: Compose,
    submit: Submit,
    retire: Retire,
    feedback: Feedback,
}

impl<Compose, Submit, Retire, Feedback>
    LiveProductionPresentationAdapter<Compose, Submit, Retire, Feedback>
{
    pub const fn new(compose: Compose, submit: Submit, retire: Retire, feedback: Feedback) -> Self {
        Self {
            compose,
            submit,
            retire,
            feedback,
        }
    }
}

impl<Compose, Submit, Retire, Feedback, Frame, Submission, Retirement, Evidence, Error>
    ProductionPresentationAdapter
    for LiveProductionPresentationAdapter<Compose, Submit, Retire, Feedback>
where
    Compose: FnMut(
        u64,
        &[CommittedSurfaceState],
        &[sophia_protocol::TransactionCommit],
    ) -> Result<Frame, Error>,
    Submit: FnMut(u64, Frame) -> Result<Submission, Error>,
    Retire: FnMut() -> Result<Vec<ProductionRetirement<Retirement>>, Error>,
    Feedback: FnMut(u64, Retirement) -> Result<Evidence, Error>,
{
    type Frame = Frame;
    type Submission = Submission;
    type Retirement = Retirement;
    type Evidence = Evidence;
    type Error = Error;

    fn compose(
        &mut self,
        cycle: u64,
        committed: &[CommittedSurfaceState],
        authority_commits: &[sophia_protocol::TransactionCommit],
    ) -> Result<Self::Frame, Self::Error> {
        (self.compose)(cycle, committed, authority_commits)
    }

    fn submit_frame(
        &mut self,
        cycle: u64,
        frame: Self::Frame,
    ) -> Result<Self::Submission, Self::Error> {
        (self.submit)(cycle, frame)
    }

    fn poll_retirements(
        &mut self,
    ) -> Result<Vec<ProductionRetirement<Self::Retirement>>, Self::Error> {
        (self.retire)()
    }

    fn route_protocol_feedback(
        &mut self,
        cycle: u64,
        retirement: Self::Retirement,
    ) -> Result<Self::Evidence, Self::Error> {
        (self.feedback)(cycle, retirement)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LiveProductionPageFlipRetirement {
    pub output: OutputId,
    pub ust: u64,
    pub msc: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveProductionPageFlipTrackerError {
    Schedule(OutputPresentationSchedule),
    Feedback(OutputPresentationFeedback),
    Retirement(OutputPresentationRetire),
    MissingCycle { output: OutputId },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveProductionPageFlipTracker {
    presentation: OutputPresentationRegistry,
    pending: BTreeMap<OutputId, (u64, u64)>,
    retirements: VecDeque<QueuedPageFlipRetirement>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct QueuedPageFlipRetirement {
    retirement: ProductionRetirement<LiveProductionPageFlipRetirement>,
    #[cfg(all(feature = "libdrm-events", feature = "gbm-probe"))]
    native: Option<crate::LiveProductionNativeRetirementContent>,
}

impl LiveProductionPageFlipTracker {
    pub fn from_outputs(outputs: &EngineHeadRegistry) -> Self {
        Self {
            presentation: OutputPresentationRegistry::from_outputs(outputs),
            pending: BTreeMap::new(),
            retirements: VecDeque::new(),
        }
    }

    pub fn submit(
        &mut self,
        output: OutputId,
        cycle: u64,
    ) -> Result<u64, LiveProductionPageFlipTrackerError> {
        let _ = self.presentation.mark_damage(output);
        match self.presentation.schedule(output) {
            OutputPresentationSchedule::Scheduled(frame) => {
                self.pending.insert(output, (cycle, frame.frame_serial));
                Ok(frame.frame_serial)
            }
            outcome => Err(LiveProductionPageFlipTrackerError::Schedule(outcome)),
        }
    }

    pub fn observe_page_flip(
        &mut self,
        output: OutputId,
        sequence: u64,
        ust: u64,
    ) -> Result<(), LiveProductionPageFlipTrackerError> {
        let (cycle, frame_serial) = self
            .pending
            .get(&output)
            .copied()
            .ok_or(LiveProductionPageFlipTrackerError::MissingCycle { output })?;
        let feedback = self
            .presentation
            .observe_page_flip(output, sequence, ust / 1_000);
        match self.presentation.retire(output, frame_serial) {
            OutputPresentationRetire::Retired { .. } => {
                // A page flip accepted by the physical owner has released
                // this exact logical submission even when its cadence sample
                // is invalid. Do not strand ownership and turn one timing
                // fault into an overlap/phase cascade.
                self.pending.remove(&output);
            }
            outcome => return Err(LiveProductionPageFlipTrackerError::Retirement(outcome)),
        }
        if !matches!(feedback, OutputPresentationFeedback::Accepted { .. }) {
            return Err(LiveProductionPageFlipTrackerError::Feedback(feedback));
        }
        self.retirements.push_back(QueuedPageFlipRetirement {
            retirement: ProductionRetirement {
                cycle,
                retirement: LiveProductionPageFlipRetirement {
                    output,
                    ust,
                    msc: sequence,
                },
            },
            #[cfg(all(feature = "libdrm-events", feature = "gbm-probe"))]
            native: None,
        });
        Ok(())
    }

    pub fn drain_retirements(
        &mut self,
    ) -> Vec<ProductionRetirement<LiveProductionPageFlipRetirement>> {
        self.retirements
            .drain(..)
            .map(|queued| queued.retirement)
            .collect()
    }

    pub fn take_retirement(
        &mut self,
        output: OutputId,
    ) -> Option<ProductionRetirement<LiveProductionPageFlipRetirement>> {
        let index = self
            .retirements
            .iter()
            .position(|queued| queued.retirement.retirement.output == output)?;
        self.retirements
            .remove(index)
            .map(|queued| queued.retirement)
    }

    pub fn discard_retirements(&mut self, output: Option<OutputId>) {
        match output {
            Some(output) => self
                .retirements
                .retain(|queued| queued.retirement.retirement.output != output),
            None => self.retirements.clear(),
        }
    }
}

#[cfg(all(feature = "libdrm-events", feature = "gbm-probe"))]
impl LiveProductionPageFlipTracker {
    pub(crate) fn observe_native_page_flip(
        &mut self,
        output: OutputId,
        sequence: u64,
        ust: u64,
        mut native: Option<crate::LiveProductionNativeRetirementContent>,
    ) -> Result<(), LiveProductionPageFlipTrackerError> {
        self.observe_page_flip(output, sequence, ust)?;
        let queued = self
            .retirements
            .back_mut()
            .expect("accepted flip queued its retirement");
        if let Some(native) = &mut native
            && native.submission != queued.retirement.cycle
        {
            native.layout_witness = None;
        }
        queued.native = native;
        Ok(())
    }

    pub(crate) fn take_native_retirement(
        &mut self,
        output: OutputId,
    ) -> Option<(
        ProductionRetirement<LiveProductionPageFlipRetirement>,
        Option<crate::LiveProductionNativeRetirementContent>,
    )> {
        let index = self
            .retirements
            .iter()
            .position(|queued| queued.retirement.retirement.output == output)?;
        self.retirements
            .remove(index)
            .map(|queued| (queued.retirement, queued.native))
    }

    pub(crate) fn invalidate_layout_witnesses(&mut self) {
        for queued in &mut self.retirements {
            if let Some(native) = &mut queued.native {
                native.layout_witness = None;
            }
        }
    }
}

#[cfg(all(feature = "libdrm-events", feature = "gbm-probe"))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LivePresentBufferDisposition {
    Copied,
    Retained,
    Skipped,
    /// The client's own buffer reached the screen by a page flip, uncomposed.
    ///
    /// Distinct from `Retained`, which also reports X `Flip` but means the
    /// previous frame stayed on glass. This one means a new client buffer is
    /// on glass and is still owed to the client until a successor retires it,
    /// which is why it completes without idling. `docs/validation.md:534`
    /// reserved the X `Flip` completion for exactly this frame.
    Flipped,
}

#[cfg(all(feature = "libdrm-events", feature = "gbm-probe"))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LivePresentProtocolFeedback {
    Complete {
        transaction: TransactionId,
        ust: u64,
        msc: u64,
        disposition: LivePresentBufferDisposition,
    },
    Idle {
        transaction: TransactionId,
    },
}

#[cfg(all(feature = "libdrm-events", feature = "gbm-probe"))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LivePresentLayoutComparison {
    pub candidate: sophia_protocol::SurfaceTransactionKey,
    pub retired: crate::LiveProductionRetiredLayoutWitness,
}

#[cfg(all(feature = "libdrm-events", feature = "gbm-probe"))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LivePresentFeedbackOutcome {
    pub feedback: Vec<LivePresentProtocolFeedback>,
    pub idle_fence_triggered: bool,
    /// Historical evidence attached to this outcome's exact Complete, never a permit.
    pub layout_comparison: Option<Box<LivePresentLayoutComparison>>,
    /// Real samples belonging to this request, across every retiring output.
    pub clocks: crate::LiveNativeRetirementClocks,
}

#[cfg(all(feature = "libdrm-events", feature = "gbm-probe"))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LivePresentFeedbackError {
    UnknownPresentation { transaction: TransactionId },
}

#[cfg(all(feature = "libdrm-events", feature = "gbm-probe"))]
impl std::fmt::Display for LivePresentFeedbackError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownPresentation { transaction } => write!(
                formatter,
                "unknown live presentation transaction {}",
                transaction.raw()
            ),
        }
    }
}

#[cfg(all(feature = "libdrm-events", feature = "gbm-probe"))]
impl std::error::Error for LivePresentFeedbackError {}

#[cfg(all(feature = "libdrm-events", feature = "gbm-probe"))]
#[derive(Debug, Default)]
pub struct LiveProductionPresentFeedbackCoordinator {
    resources: crate::LivePresentationResourceSession,
    last_display_sample: Option<(u64, u64)>,
}

#[cfg(all(feature = "libdrm-events", feature = "gbm-probe"))]
impl LiveProductionPresentFeedbackCoordinator {
    pub fn resources(&self) -> &crate::LivePresentationResourceSession {
        &self.resources
    }

    pub fn resources_mut(&mut self) -> &mut crate::LivePresentationResourceSession {
        &mut self.resources
    }

    pub fn observe_authority_resources(
        &mut self,
        batch: &crate::LiveProductionAuthorityBatch,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.observe_authority_resource_registrations(batch)?;
        self.observe_authority_resource_releases(batch);
        Ok(())
    }

    pub fn observe_authority_resource_registrations(
        &mut self,
        batch: &crate::LiveProductionAuthorityBatch,
    ) -> Result<(), Box<dyn std::error::Error>> {
        for registration in &batch.dma_buf_registrations {
            let plane_fds = registration
                .plane_fds
                .iter()
                .map(|fd| fd.as_ref().try_clone())
                .collect::<Result<Vec<_>, _>>()?;
            self.resources
                .register_source(registration.descriptor, plane_fds)?;
        }
        for registration in &batch.fence_registrations {
            self.resources.register_fence(
                registration.handle,
                registration.initially_triggered,
                registration.fd.as_ref().try_clone()?,
            )?;
        }
        Ok(())
    }

    pub fn observe_authority_resource_releases(
        &mut self,
        batch: &crate::LiveProductionAuthorityBatch,
    ) {
        for handle in &batch.released_dma_bufs {
            let _ = self.resources.release_source(*handle);
        }
        for handle in &batch.released_fences {
            let _ = self.resources.release_fence(*handle);
        }
    }

    pub fn complete_copy(
        &mut self,
        transaction: TransactionId,
        ust: u64,
        msc: u64,
    ) -> Result<LivePresentFeedbackOutcome, LivePresentFeedbackError> {
        let retirement = self
            .resources
            .retire_page_flip(transaction)
            .ok_or(LivePresentFeedbackError::UnknownPresentation { transaction })?;
        self.last_display_sample = Some((ust, msc));
        Ok(Self::outcome(
            transaction,
            ust,
            msc,
            LivePresentBufferDisposition::Copied,
            retirement.idle_fence == sophia_renderer_live::LiveIdleFenceStatus::Triggered,
        ))
    }

    pub fn complete_retained_without_idle(
        &mut self,
        transaction: TransactionId,
        ust: u64,
        msc: u64,
    ) -> Result<LivePresentFeedbackOutcome, LivePresentFeedbackError> {
        if self.resources.state(transaction)
            != Some(sophia_renderer_live::LiveBufferState::Submitted)
        {
            return Err(LivePresentFeedbackError::UnknownPresentation { transaction });
        }
        self.last_display_sample = Some((ust, msc));
        Ok(LivePresentFeedbackOutcome {
            feedback: vec![LivePresentProtocolFeedback::Complete {
                transaction,
                ust,
                msc,
                disposition: LivePresentBufferDisposition::Retained,
            }],
            idle_fence_triggered: false,
            layout_comparison: None,
            clocks: Default::default(),
        })
    }

    /// Complete a Present whose own buffer is on the screen.
    ///
    /// The page flip is not retired here, because retiring it would release a
    /// buffer the screen is scanning and let the client draw into displayed
    /// pixels. The release happens in `idle_displayed` once a successor flip
    /// has taken the plane. See `PresentFlipOwnership.tla`,
    /// `DisplayedClientBufferIsNeverReleased` and `ReleasedOnlyBySuccessor`.
    pub fn complete_flip_without_idle(
        &mut self,
        transaction: TransactionId,
        ust: u64,
        msc: u64,
    ) -> Result<LivePresentFeedbackOutcome, LivePresentFeedbackError> {
        if self.resources.state(transaction)
            != Some(sophia_renderer_live::LiveBufferState::Submitted)
        {
            return Err(LivePresentFeedbackError::UnknownPresentation { transaction });
        }
        self.last_display_sample = Some((ust, msc));
        Ok(LivePresentFeedbackOutcome {
            feedback: vec![LivePresentProtocolFeedback::Complete {
                transaction,
                ust,
                msc,
                disposition: LivePresentBufferDisposition::Flipped,
            }],
            idle_fence_triggered: false,
            layout_comparison: None,
            clocks: Default::default(),
        })
    }

    pub fn idle_displayed(
        &mut self,
        transaction: TransactionId,
    ) -> Result<LivePresentFeedbackOutcome, LivePresentFeedbackError> {
        let retirement = self
            .resources
            .retire_page_flip(transaction)
            .ok_or(LivePresentFeedbackError::UnknownPresentation { transaction })?;
        Ok(LivePresentFeedbackOutcome {
            feedback: vec![LivePresentProtocolFeedback::Idle { transaction }],
            idle_fence_triggered: retirement.idle_fence
                == sophia_renderer_live::LiveIdleFenceStatus::Triggered,
            layout_comparison: None,
            clocks: Default::default(),
        })
    }

    pub fn reject_skip(
        &mut self,
        transaction: TransactionId,
        ust: u64,
        msc: u64,
    ) -> Result<LivePresentFeedbackOutcome, LivePresentFeedbackError> {
        let retirement = self
            .resources
            .reject(transaction)
            .ok_or(LivePresentFeedbackError::UnknownPresentation { transaction })?;
        if ust != 0 || msc != 0 {
            self.last_display_sample = Some((ust, msc));
        }
        Ok(Self::outcome(
            transaction,
            ust,
            msc,
            LivePresentBufferDisposition::Skipped,
            retirement.idle_fence == sophia_renderer_live::LiveIdleFenceStatus::Triggered,
        ))
    }

    pub fn reject_skip_at_last_display(
        &mut self,
        transaction: TransactionId,
    ) -> Result<LivePresentFeedbackOutcome, LivePresentFeedbackError> {
        let (ust, msc) = self.last_display_sample.unwrap_or_default();
        self.reject_skip(transaction, ust, msc)
    }

    pub fn disconnect(&mut self) -> sophia_renderer_live::LivePresentationDisconnectReport {
        self.resources.disconnect()
    }

    fn outcome(
        transaction: TransactionId,
        ust: u64,
        msc: u64,
        disposition: LivePresentBufferDisposition,
        idle_fence_triggered: bool,
    ) -> LivePresentFeedbackOutcome {
        let complete = LivePresentProtocolFeedback::Complete {
            transaction,
            ust,
            msc,
            disposition,
        };
        let idle = LivePresentProtocolFeedback::Idle { transaction };
        LivePresentFeedbackOutcome {
            // X Present Copy releases the source after the copy finishes and
            // completes against the display clock afterward. Mesa relies on
            // seeing Idle before Complete when both become known together.
            feedback: if disposition == LivePresentBufferDisposition::Copied {
                vec![idle, complete]
            } else {
                vec![complete, idle]
            },
            idle_fence_triggered,
            layout_comparison: None,
            clocks: Default::default(),
        }
    }
}

mod head_table;
pub use head_table::*;

#[cfg(all(feature = "libdrm-events", feature = "gbm-probe"))]
mod native_scanout;
#[cfg(all(feature = "libdrm-events", feature = "gbm-probe"))]
pub use native_scanout::*;
