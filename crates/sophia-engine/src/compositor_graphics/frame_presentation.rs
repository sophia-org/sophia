#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutputFramePresentation {
    pub snapshot: OutputFrameDamageSnapshot,
    pub compositor_damage: Region,
    pub damage: Region,
    pub repaint: OutputRepaintPlan,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutputRepaintPolicy {
    pub max_partial_rects: usize,
    pub full_repaint_percent: u8,
}

impl Default for OutputRepaintPolicy {
    fn default() -> Self {
        Self {
            max_partial_rects: 32,
            full_repaint_percent: 60,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputFullRepaintReason {
    DamageCapacityExceeded,
    PartialRectLimitExceeded,
    CoverageThresholdReached,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OutputRepaintPlan {
    Skip,
    Partial {
        damage: Region,
        damaged_pixels: u64,
    },
    Full {
        damage: Region,
        damaged_pixels: u64,
        reason: OutputFullRepaintReason,
    },
}

impl OutputRepaintPlan {
    pub const fn reduced_name(&self) -> &'static str {
        match self {
            Self::Skip => "skip",
            Self::Partial { .. } => "partial",
            Self::Full { .. } => "full",
        }
    }

    pub fn damage(&self) -> Option<&Region> {
        match self {
            Self::Skip => None,
            Self::Partial { damage, .. } | Self::Full { damage, .. } => Some(damage),
        }
    }

    pub const fn damaged_pixels(&self) -> u64 {
        match self {
            Self::Skip => 0,
            Self::Partial { damaged_pixels, .. } | Self::Full { damaged_pixels, .. } => {
                *damaged_pixels
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputRepaintPlanError {
    InvalidOutputSize,
    InvalidPolicy,
}

impl fmt::Display for OutputRepaintPlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for OutputRepaintPlanError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputFramePresentationError {
    InvalidOutput,
    InvalidOutputSize,
    InvalidRepaintPolicy,
    InvalidSnapshot,
    OutputMismatch,
    MissingPending,
    RenderingInFlight,
    MissingRendering,
    SubmissionInFlight,
    MissingSubmitted,
}

impl fmt::Display for OutputFramePresentationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for OutputFramePresentationError {}

/// Tracks immutable output-frame state through the scanout lifecycle.
///
/// A queued snapshot is compared with the state that will precede it on
/// screen: the submitted snapshot when a page flip is in flight, otherwise the
/// presented snapshot. Failed or superseded queue work never advances
/// presented state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutputFramePresentationState {
    output: HeadlessOutput,
    repaint_policy: OutputRepaintPolicy,
    pending: Option<OutputFramePresentation>,
    rendering: Option<OutputFramePresentation>,
    submitted: Option<OutputFramePresentation>,
    presented: Option<OutputFrameDamageSnapshot>,
}

impl OutputFramePresentationState {
    pub fn new(output: HeadlessOutput) -> Result<Self, OutputFramePresentationError> {
        Self::with_repaint_policy(output, OutputRepaintPolicy::default())
    }

    pub fn with_repaint_policy(
        output: HeadlessOutput,
        repaint_policy: OutputRepaintPolicy,
    ) -> Result<Self, OutputFramePresentationError> {
        if !output.id.is_valid() {
            return Err(OutputFramePresentationError::InvalidOutput);
        }
        validate_output_repaint_inputs(output.size, repaint_policy).map_err(
            |error| match error {
                OutputRepaintPlanError::InvalidOutputSize => {
                    OutputFramePresentationError::InvalidOutputSize
                }
                OutputRepaintPlanError::InvalidPolicy => {
                    OutputFramePresentationError::InvalidRepaintPolicy
                }
            },
        )?;
        Ok(Self {
            output,
            repaint_policy,
            pending: None,
            rendering: None,
            submitted: None,
            presented: None,
        })
    }

    pub const fn output(&self) -> OutputId {
        self.output.id
    }

    pub fn queue(
        &mut self,
        snapshot: OutputFrameDamageSnapshot,
    ) -> Result<&OutputFramePresentation, OutputFramePresentationError> {
        if snapshot.output != self.output {
            return Err(OutputFramePresentationError::OutputMismatch);
        }
        let baseline = self
            .submitted
            .as_ref()
            .map(|submitted| &submitted.snapshot)
            .or_else(|| self.rendering.as_ref().map(|rendering| &rendering.snapshot))
            .or(self.presented.as_ref());
        let compositor_baseline = baseline.map(|baseline| &baseline.compositor_display_list);
        let compositor_damage = compositor_baseline.map_or_else(
            || {
                let empty = CompositorDamageList::empty(self.output.id);
                compositor_display_list_damage(&empty, &snapshot.compositor_display_list)
            },
            |baseline| compositor_display_list_damage(baseline, &snapshot.compositor_display_list),
        );
        let damage = output_frame_damage(baseline, &snapshot).map_err(|error| match error {
            crate::OutputFrameDamageError::OutputMismatch => {
                OutputFramePresentationError::OutputMismatch
            }
            _ => OutputFramePresentationError::InvalidSnapshot,
        })?;
        let repaint = plan_output_repaint(self.output.size, &damage, self.repaint_policy)
            .expect("presentation state validates its output and repaint policy");
        self.pending = Some(OutputFramePresentation {
            snapshot,
            compositor_damage,
            damage,
            repaint,
        });
        Ok(self.pending.as_ref().expect("assigned above"))
    }

    pub fn discard_pending(&mut self) -> Option<OutputFramePresentation> {
        self.pending.take()
    }

    pub fn mark_rendering(
        &mut self,
    ) -> Result<&OutputFramePresentation, OutputFramePresentationError> {
        if self.rendering.is_some() {
            return Err(OutputFramePresentationError::RenderingInFlight);
        }
        // Rendering owns a different native target from the submitted frame.
        // Keeping both slots live lets a mirror head prepare its successor
        // while the current KMS commit waits for that head's vblank.
        self.rendering = Some(
            self.pending
                .take()
                .ok_or(OutputFramePresentationError::MissingPending)?,
        );
        Ok(self.rendering.as_ref().expect("assigned above"))
    }

    pub fn promote_rendering_to_submitted(
        &mut self,
    ) -> Result<&OutputFramePresentation, OutputFramePresentationError> {
        if self.submitted.is_some() {
            return Err(OutputFramePresentationError::SubmissionInFlight);
        }
        self.submitted = Some(
            self.rendering
                .take()
                .ok_or(OutputFramePresentationError::MissingRendering)?,
        );
        Ok(self.submitted.as_ref().expect("assigned above"))
    }

    pub fn discard_rendering(&mut self) -> Option<OutputFramePresentation> {
        self.rendering.take()
    }

    pub fn mark_submitted(
        &mut self,
    ) -> Result<&OutputFramePresentation, OutputFramePresentationError> {
        if self.rendering.is_some() {
            return Err(OutputFramePresentationError::RenderingInFlight);
        }
        if self.submitted.is_some() {
            return Err(OutputFramePresentationError::SubmissionInFlight);
        }
        self.submitted = Some(
            self.pending
                .take()
                .ok_or(OutputFramePresentationError::MissingPending)?,
        );
        Ok(self.submitted.as_ref().expect("assigned above"))
    }

    pub fn mark_presented(
        &mut self,
    ) -> Result<OutputFramePresentation, OutputFramePresentationError> {
        let submitted = self
            .submitted
            .take()
            .ok_or(OutputFramePresentationError::MissingSubmitted)?;
        self.presented = Some(submitted.snapshot.clone());
        Ok(submitted)
    }

    pub fn mark_initial_presented(
        &mut self,
    ) -> Result<OutputFramePresentation, OutputFramePresentationError> {
        if self.rendering.is_some() {
            return Err(OutputFramePresentationError::RenderingInFlight);
        }
        if self.submitted.is_some() {
            return Err(OutputFramePresentationError::SubmissionInFlight);
        }
        let pending = self
            .pending
            .take()
            .ok_or(OutputFramePresentationError::MissingPending)?;
        self.presented = Some(pending.snapshot.clone());
        Ok(pending)
    }

    pub fn pending(&self) -> Option<&OutputFramePresentation> {
        self.pending.as_ref()
    }

    pub fn rendering(&self) -> Option<&OutputFramePresentation> {
        self.rendering.as_ref()
    }

    pub fn submitted(&self) -> Option<&OutputFramePresentation> {
        self.submitted.as_ref()
    }

    pub fn presented(&self) -> Option<&OutputFrameDamageSnapshot> {
        self.presented.as_ref()
    }
}

/// Reduces raw compositor-node damage into bounded output-local repaint work.
///
/// Rectangles are clipped to the output and exact rectangular unions are
/// coalesced deterministically. Excess complexity or coverage falls back to a
/// full repaint; incomplete proof therefore costs performance, never pixels.
pub fn plan_output_repaint(
    output_size: Size,
    damage: &Region,
    policy: OutputRepaintPolicy,
) -> Result<OutputRepaintPlan, OutputRepaintPlanError> {
    validate_output_repaint_inputs(output_size, policy)?;
    let full_output = Rect {
        x: 0,
        y: 0,
        width: output_size.width,
        height: output_size.height,
    };
    let output_pixels = rect_area(full_output);
    if damage.rects.len() > MAX_OUTPUT_DAMAGE_RECTS {
        return Ok(OutputRepaintPlan::Full {
            damage: Region::single(full_output),
            damaged_pixels: output_pixels,
            reason: OutputFullRepaintReason::DamageCapacityExceeded,
        });
    }

    let mut rects = Vec::with_capacity(damage.rects.len());
    for rect in damage.rects.iter().copied() {
        let Some(mut current) = clip_rect(rect, full_output) else {
            continue;
        };
        let mut index = 0;
        while index < rects.len() {
            if rects_form_rectangle(current, rects[index]) {
                current = bounding_rect(current, rects.swap_remove(index));
                index = 0;
            } else {
                index += 1;
            }
        }
        rects.push(current);
    }
    rects.sort_by_key(|rect| (rect.y, rect.x, rect.height, rect.width));
    if rects.is_empty() {
        return Ok(OutputRepaintPlan::Skip);
    }
    if rects.len() > policy.max_partial_rects {
        return Ok(OutputRepaintPlan::Full {
            damage: Region::single(full_output),
            damaged_pixels: output_pixels,
            reason: OutputFullRepaintReason::PartialRectLimitExceeded,
        });
    }

    let damaged_pixels = rects
        .iter()
        .copied()
        .map(rect_area)
        .fold(0_u64, u64::saturating_add);
    if damaged_pixels.saturating_mul(100)
        >= output_pixels.saturating_mul(u64::from(policy.full_repaint_percent))
    {
        return Ok(OutputRepaintPlan::Full {
            damage: Region::single(full_output),
            damaged_pixels: output_pixels,
            reason: OutputFullRepaintReason::CoverageThresholdReached,
        });
    }
    Ok(OutputRepaintPlan::Partial {
        damage: Region { rects },
        damaged_pixels,
    })
}

fn validate_output_repaint_inputs(
    output_size: Size,
    policy: OutputRepaintPolicy,
) -> Result<(), OutputRepaintPlanError> {
    if output_size.width <= 0 || output_size.height <= 0 {
        return Err(OutputRepaintPlanError::InvalidOutputSize);
    }
    if policy.max_partial_rects == 0
        || policy.max_partial_rects > MAX_OUTPUT_DAMAGE_RECTS
        || !(1..=100).contains(&policy.full_repaint_percent)
    {
        return Err(OutputRepaintPlanError::InvalidPolicy);
    }
    Ok(())
}

fn clip_rect(rect: Rect, bounds: Rect) -> Option<Rect> {
    let x = rect.x.max(bounds.x);
    let y = rect.y.max(bounds.y);
    let right = rect
        .x
        .saturating_add(rect.width)
        .min(bounds.x.saturating_add(bounds.width));
    let bottom = rect
        .y
        .saturating_add(rect.height)
        .min(bounds.y.saturating_add(bounds.height));
    let clipped = Rect {
        x,
        y,
        width: right.saturating_sub(x),
        height: bottom.saturating_sub(y),
    };
    (!clipped.is_empty()).then_some(clipped)
}

fn rects_form_rectangle(left: Rect, right: Rect) -> bool {
    let bounds = bounding_rect(left, right);
    let intersection = clip_rect(left, right).map_or(0, rect_area);
    rect_area(bounds)
        == rect_area(left)
            .saturating_add(rect_area(right))
            .saturating_sub(intersection)
}

fn bounding_rect(left: Rect, right: Rect) -> Rect {
    let x = left.x.min(right.x);
    let y = left.y.min(right.y);
    Rect {
        x,
        y,
        width: left
            .x
            .saturating_add(left.width)
            .max(right.x.saturating_add(right.width))
            .saturating_sub(x),
        height: left
            .y
            .saturating_add(left.height)
            .max(right.y.saturating_add(right.height))
            .saturating_sub(y),
    }
}

fn rect_area(rect: Rect) -> u64 {
    u64::try_from(rect.width)
        .unwrap_or_default()
        .saturating_mul(u64::try_from(rect.height).unwrap_or_default())
}
