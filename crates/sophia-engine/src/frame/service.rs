use crate::prelude::*;

pub const OUTPUT_FRAME_SERVICE_CAPACITY: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputNativeFramePhase {
    Idle,
    InFlight,
    CleanupPending,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutputFrameServiceObservation {
    pub output: OutputId,
    pub primary: bool,
    pub native_phase: OutputNativeFramePhase,
    /// Native scanout work owed to this output alone. A waiting software
    /// present is request-global rather than per-output, so it belongs to
    /// `software_frame_waiting` and not here: folding it in made every output
    /// look busy and hid which one actually owed a frame.
    pub pending_frame: bool,
}

const UNUSED_OBSERVATION: OutputFrameServiceObservation = OutputFrameServiceObservation {
    output: OutputId::from_raw(0),
    primary: false,
    native_phase: OutputNativeFramePhase::Idle,
    pending_frame: false,
};

/// Observations stored inline, so the owner can build one per service pass
/// without a heap allocation.
///
/// Observations past the capacity are not stored but are still counted. The
/// validator then refuses the request with the same `CapacityExceeded` count
/// an unbounded list would have produced.
#[derive(Clone, Copy)]
pub struct OutputFrameServiceObservations {
    items: [OutputFrameServiceObservation; OUTPUT_FRAME_SERVICE_CAPACITY],
    len: usize,
    offered: usize,
}

impl OutputFrameServiceObservations {
    pub const fn new() -> Self {
        Self {
            items: [UNUSED_OBSERVATION; OUTPUT_FRAME_SERVICE_CAPACITY],
            len: 0,
            offered: 0,
        }
    }

    pub fn push(&mut self, observation: OutputFrameServiceObservation) {
        if self.len < OUTPUT_FRAME_SERVICE_CAPACITY {
            self.items[self.len] = observation;
            self.len += 1;
        }
        self.offered = self.offered.saturating_add(1);
    }

    /// Every observation pushed, including any beyond the capacity.
    pub const fn offered(&self) -> usize {
        self.offered
    }
}

impl Default for OutputFrameServiceObservations {
    fn default() -> Self {
        Self::new()
    }
}

impl std::ops::Deref for OutputFrameServiceObservations {
    type Target = [OutputFrameServiceObservation];

    fn deref(&self) -> &Self::Target {
        &self.items[..self.len]
    }
}

impl std::ops::DerefMut for OutputFrameServiceObservations {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.items[..self.len]
    }
}

impl PartialEq for OutputFrameServiceObservations {
    fn eq(&self, other: &Self) -> bool {
        self.offered == other.offered && **self == **other
    }
}

impl Eq for OutputFrameServiceObservations {}

impl std::fmt::Debug for OutputFrameServiceObservations {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.offered == self.len {
            f.debug_list().entries(self.iter()).finish()
        } else {
            f.debug_struct("OutputFrameServiceObservations")
                .field("stored", &&**self)
                .field("offered", &self.offered)
                .finish()
        }
    }
}

impl Extend<OutputFrameServiceObservation> for OutputFrameServiceObservations {
    fn extend<I: IntoIterator<Item = OutputFrameServiceObservation>>(&mut self, iter: I) {
        for observation in iter {
            self.push(observation);
        }
    }
}

impl FromIterator<OutputFrameServiceObservation> for OutputFrameServiceObservations {
    fn from_iter<I: IntoIterator<Item = OutputFrameServiceObservation>>(iter: I) -> Self {
        let mut observations = Self::new();
        observations.extend(iter);
        observations
    }
}

impl<'a> IntoIterator for &'a OutputFrameServiceObservations {
    type Item = &'a OutputFrameServiceObservation;
    type IntoIter = std::slice::Iter<'a, OutputFrameServiceObservation>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a> IntoIterator for &'a mut OutputFrameServiceObservations {
    type Item = &'a mut OutputFrameServiceObservation;
    type IntoIter = std::slice::IterMut<'a, OutputFrameServiceObservation>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct OutputFrameServiceRequest {
    pub outputs: OutputFrameServiceObservations,
    pub presentation_queued: bool,
    /// A composed software present is waiting to be lowered onto head frames.
    /// Staging it requires every output to be natively idle, so it is a
    /// property of the request rather than of any one output.
    pub software_frame_waiting: bool,
    /// Renderer preparation can progress on the next owner pass, but has no
    /// native frame to submit yet. Busy workers wake the owner themselves.
    pub preparation_pending: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputFrameServiceEffect {
    PollRetirement { output: OutputId },
    SubmitQueuedPresentation { output: OutputId },
    SubmitPendingFrame { output: OutputId },
}

impl OutputFrameServiceEffect {
    pub const fn output(self) -> OutputId {
        match self {
            Self::PollRetirement { output }
            | Self::SubmitQueuedPresentation { output }
            | Self::SubmitPendingFrame { output } => output,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputFrameServiceError {
    EmptyOutputSet,
    CapacityExceeded {
        capacity: usize,
        actual: usize,
    },
    DuplicateOutput {
        output: OutputId,
    },
    MissingPrimary,
    MultiplePrimary,
    OutputSetChanged,
    PrimaryChanged {
        expected: OutputId,
        actual: OutputId,
    },
    EffectBudgetExhausted {
        budget: usize,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutputFrameServiceReducer {
    outputs: OutputSet,
    primary: OutputId,
    retirement_attempted: OutputSet,
    pending_frame_attempted: OutputSet,
    presentation_attempted: bool,
    effect_budget: usize,
    effects_emitted: usize,
}

impl OutputFrameServiceReducer {
    pub fn begin(request: &OutputFrameServiceRequest) -> Result<Self, OutputFrameServiceError> {
        let effect_budget = request.outputs.len().saturating_mul(2).saturating_add(1);
        Self::with_effect_budget(request, effect_budget)
    }

    pub fn with_effect_budget(
        request: &OutputFrameServiceRequest,
        effect_budget: usize,
    ) -> Result<Self, OutputFrameServiceError> {
        let (outputs, primary) = validate_output_frame_service_request(request)?;
        Ok(Self {
            outputs,
            primary,
            retirement_attempted: OutputSet::new(),
            pending_frame_attempted: OutputSet::new(),
            presentation_attempted: false,
            effect_budget,
            effects_emitted: 0,
        })
    }

    pub const fn primary(&self) -> OutputId {
        self.primary
    }

    pub const fn effects_emitted(&self) -> usize {
        self.effects_emitted
    }

    pub fn next_effect(
        &mut self,
        request: &OutputFrameServiceRequest,
    ) -> Result<Option<OutputFrameServiceEffect>, OutputFrameServiceError> {
        self.validate_stable_output_set(request)?;

        if let Some(output) = request
            .outputs
            .iter()
            .filter(|output| output.native_phase != OutputNativeFramePhase::Idle)
            .map(|output| output.output)
            .filter(|output| !self.retirement_attempted.contains(output))
            .min()
        {
            self.retirement_attempted.insert(output);
            return self.emit(OutputFrameServiceEffect::PollRetirement { output });
        }

        let primary = request
            .outputs
            .iter()
            .find(|output| output.output == self.primary)
            .expect("stable validated output set contains its primary");
        // A present is emitted only when its handler can actually execute it.
        // The handler refuses while the primary owes a native frame, and it
        // used to do so silently, leaving the present queued forever; the
        // effect that drains that frame was in turn withheld while a present
        // was queued, so the two waited on each other permanently.
        if request.presentation_queued
            && primary.native_phase == OutputNativeFramePhase::Idle
            && !primary.pending_frame
            && !self.presentation_attempted
        {
            self.presentation_attempted = true;
            return self.emit(OutputFrameServiceEffect::SubmitQueuedPresentation {
                output: self.primary,
            });
        }

        if let Some(output) = request
            .outputs
            .iter()
            .filter(|output| {
                output.native_phase == OutputNativeFramePhase::Idle && output.pending_frame
            })
            .map(|output| output.output)
            .filter(|output| !self.pending_frame_attempted.contains(output))
            .min()
        {
            self.pending_frame_attempted.insert(output);
            return self.emit(OutputFrameServiceEffect::SubmitPendingFrame { output });
        }

        // Lowering a waiting software present onto head frames requires every
        // output to be natively idle, not merely the one it targets, and its
        // handler fails the session outright rather than deferring when that
        // does not hold. Emitting it only under the stronger condition is what
        // keeps that failure unreachable from here.
        if request.software_frame_waiting
            && !self.pending_frame_attempted.contains(&self.primary)
            && request.outputs.iter().all(|output| {
                output.native_phase == OutputNativeFramePhase::Idle && !output.pending_frame
            })
        {
            self.pending_frame_attempted.insert(self.primary);
            return self.emit(OutputFrameServiceEffect::SubmitPendingFrame {
                output: self.primary,
            });
        }

        Ok(None)
    }

    fn emit(
        &mut self,
        effect: OutputFrameServiceEffect,
    ) -> Result<Option<OutputFrameServiceEffect>, OutputFrameServiceError> {
        if self.effects_emitted >= self.effect_budget {
            return Err(OutputFrameServiceError::EffectBudgetExhausted {
                budget: self.effect_budget,
            });
        }
        self.effects_emitted = self.effects_emitted.saturating_add(1);
        Ok(Some(effect))
    }

    fn validate_stable_output_set(
        &self,
        request: &OutputFrameServiceRequest,
    ) -> Result<(), OutputFrameServiceError> {
        let (outputs, primary) = validate_output_frame_service_request(request)?;
        if outputs != self.outputs {
            return Err(OutputFrameServiceError::OutputSetChanged);
        }
        if primary != self.primary {
            return Err(OutputFrameServiceError::PrimaryChanged {
                expected: self.primary,
                actual: primary,
            });
        }
        Ok(())
    }
}

fn validate_output_frame_service_request(
    request: &OutputFrameServiceRequest,
) -> Result<(OutputSet, OutputId), OutputFrameServiceError> {
    // `offered` counts observations the inline storage could not keep.
    if request.outputs.offered() == 0 {
        return Err(OutputFrameServiceError::EmptyOutputSet);
    }
    if request.outputs.offered() > OUTPUT_FRAME_SERVICE_CAPACITY {
        return Err(OutputFrameServiceError::CapacityExceeded {
            capacity: OUTPUT_FRAME_SERVICE_CAPACITY,
            actual: request.outputs.offered(),
        });
    }

    let mut outputs = OutputSet::new();
    let mut primary = None;
    for observation in &request.outputs {
        if !outputs.insert(observation.output) {
            return Err(OutputFrameServiceError::DuplicateOutput {
                output: observation.output,
            });
        }
        if observation.primary && primary.replace(observation.output).is_some() {
            return Err(OutputFrameServiceError::MultiplePrimary);
        }
    }
    let primary = primary.ok_or(OutputFrameServiceError::MissingPrimary)?;
    Ok((outputs, primary))
}

/// The request validator bounds every set, including attempted effects.
/// Keep IDs sorted so a reordered observation compares equal without a heap set.
#[derive(Clone, Debug, Eq, PartialEq)]
struct OutputSet {
    ids: [OutputId; OUTPUT_FRAME_SERVICE_CAPACITY],
    len: usize,
}

impl OutputSet {
    fn new() -> Self {
        Self {
            ids: [OutputId::from_raw(0); OUTPUT_FRAME_SERVICE_CAPACITY],
            len: 0,
        }
    }

    fn contains(&self, output: &OutputId) -> bool {
        self.ids[..self.len].binary_search(output).is_ok()
    }

    fn insert(&mut self, output: OutputId) -> bool {
        let Err(index) = self.ids[..self.len].binary_search(&output) else {
            return false;
        };
        assert!(
            self.len < OUTPUT_FRAME_SERVICE_CAPACITY,
            "validated output set capacity"
        );
        self.ids.copy_within(index..self.len, index + 1);
        self.ids[index] = output;
        self.len += 1;
        true
    }
}
