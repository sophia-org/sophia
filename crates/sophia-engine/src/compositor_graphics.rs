use crate::prelude::*;
use crate::{HeadlessOutput, IndicatorChromeStrip, OutputFrameDamageSnapshot, output_frame_damage};

#[path = "compositor_graphics/chrome_layout.rs"]
mod chrome_layout;
pub use chrome_layout::*;
#[path = "compositor_graphics/chrome_summary.rs"]
mod chrome_summary;
pub use chrome_summary::*;

// Includes surface chrome, 2,048 member labels, 1,024 empty-cell labels,
// indicators and the descriptor switcher, all within one explicit bound.
pub const MAX_COMPOSITOR_DISPLAY_COMMANDS: usize = 10_240;
pub const MAX_OUTPUT_DAMAGE_RECTS: usize = MAX_COMPOSITOR_DISPLAY_COMMANDS * 2;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum SurfaceChromeRole {
    Frame,
    FocusRing,
    FloatingOutline,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum DescriptorOverlayNodeRole {
    Panel,
    Row,
    Selection,
    Trust,
    Attention,
    Label,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum CompositorNodeId {
    SurfaceChrome {
        surface: SurfaceId,
        role: SurfaceChromeRole,
    },
    IndicatorStrip {
        output: OutputId,
    },
    TabBar {
        output: OutputId,
        group: u64,
        slot: u16,
        label: bool,
    },
    DescriptorOverlay {
        projection: u64,
        slot: u16,
        role: DescriptorOverlayNodeRole,
    },
    ShellContent {
        output: OutputId,
        candidate: u64,
        surface: u16,
        placement: u16,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompositorRgb8 {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompositorSolidRect {
    pub geometry: Rect,
    pub color: CompositorRgb8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompositorRect {
    pub opacity: u8,
    pub node: CompositorNodeId,
    pub generation: u64,
    pub geometry: Rect,
    pub color: CompositorRgb8,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompositorText {
    pub node: CompositorNodeId,
    pub generation: u64,
    pub geometry: Rect,
    pub text: String,
    pub font_size_millis: u32,
    pub color: CompositorRgb8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompositorBorder {
    pub node: CompositorNodeId,
    pub generation: u64,
    pub outer: Rect,
    pub inner: Rect,
    pub color: CompositorRgb8,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompositorIndicatorStrip {
    pub node: CompositorNodeId,
    pub generation: u64,
    pub strip: IndicatorChromeStrip,
}

/// One immutable shell resource placed in output-local physical pixels.
/// The lease is carried through every native frame clone so resource release
/// cannot precede the last scanout reference.
#[derive(Clone)]
pub struct CompositorContentImage {
    pub node: CompositorNodeId,
    pub generation: u64,
    pub output_size_px: Size,
    pub geometry_px: Rect,
    pub size_px: Size,
    pub stride: u32,
    pub format: u32,
    pub resource: sophia_runtime::ContentResourceLease,
}

impl core::fmt::Debug for CompositorContentImage {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("CompositorContentImage")
            .field("node", &self.node)
            .field("generation", &self.generation)
            .field("output_size_px", &self.output_size_px)
            .field("geometry_px", &self.geometry_px)
            .field("size_px", &self.size_px)
            .field("stride", &self.stride)
            .field("format", &self.format)
            .finish_non_exhaustive()
    }
}

impl PartialEq for CompositorContentImage {
    fn eq(&self, other: &Self) -> bool {
        self.node == other.node
            && self.generation == other.generation
            && self.output_size_px == other.output_size_px
            && self.geometry_px == other.geometry_px
            && self.size_px == other.size_px
            && self.stride == other.stride
            && self.format == other.format
            && self.resource.description() == other.resource.description()
    }
}

impl Eq for CompositorContentImage {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CompositorDisplayCommand {
    Surface { surface: SurfaceId },
    Border(CompositorBorder),
    Rect(CompositorRect),
    Text(CompositorText),
    IndicatorStrip(CompositorIndicatorStrip),
    ContentImage(CompositorContentImage),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompositorDisplayList {
    pub output: OutputId,
    pub commands: Vec<CompositorDisplayCommand>,
}

impl CompositorDisplayList {
    pub fn empty(output: OutputId) -> Self {
        Self {
            output,
            commands: Vec::new(),
        }
    }

    pub fn borders(&self) -> impl Iterator<Item = CompositorBorder> + '_ {
        self.commands.iter().filter_map(|command| match command {
            CompositorDisplayCommand::Border(border) => Some(*border),
            CompositorDisplayCommand::Surface { .. }
            | CompositorDisplayCommand::Rect(_)
            | CompositorDisplayCommand::Text(_)
            | CompositorDisplayCommand::IndicatorStrip(_)
            | CompositorDisplayCommand::ContentImage(_) => None,
        })
    }

    pub fn rects(&self) -> impl Iterator<Item = CompositorRect> + '_ {
        self.commands.iter().filter_map(|command| match command {
            CompositorDisplayCommand::Rect(rect) => Some(*rect),
            CompositorDisplayCommand::Surface { .. }
            | CompositorDisplayCommand::Border(_)
            | CompositorDisplayCommand::Text(_)
            | CompositorDisplayCommand::IndicatorStrip(_)
            | CompositorDisplayCommand::ContentImage(_) => None,
        })
    }

    pub fn texts(&self) -> impl Iterator<Item = &CompositorText> + '_ {
        self.commands.iter().filter_map(|command| match command {
            CompositorDisplayCommand::Text(text) => Some(text),
            CompositorDisplayCommand::Surface { .. }
            | CompositorDisplayCommand::Border(_)
            | CompositorDisplayCommand::Rect(_)
            | CompositorDisplayCommand::IndicatorStrip(_)
            | CompositorDisplayCommand::ContentImage(_) => None,
        })
    }

    pub fn indicator_strips(&self) -> impl Iterator<Item = &CompositorIndicatorStrip> + '_ {
        self.commands.iter().filter_map(|command| match command {
            CompositorDisplayCommand::IndicatorStrip(strip) => Some(strip),
            CompositorDisplayCommand::Surface { .. }
            | CompositorDisplayCommand::Border(_)
            | CompositorDisplayCommand::Rect(_)
            | CompositorDisplayCommand::Text(_)
            | CompositorDisplayCommand::ContentImage(_) => None,
        })
    }

    pub fn content_images(&self) -> impl Iterator<Item = &CompositorContentImage> + '_ {
        self.commands.iter().filter_map(|command| match command {
            CompositorDisplayCommand::ContentImage(image) => Some(image),
            _ => None,
        })
    }
}

pub(crate) fn compositor_display_list_structure_is_valid(
    display_list: &CompositorDisplayList,
) -> bool {
    if display_list.commands.len() > MAX_COMPOSITOR_DISPLAY_COMMANDS {
        return false;
    }
    let mut nodes = BTreeSet::new();
    display_list.commands.iter().all(|command| match command {
        CompositorDisplayCommand::Surface { .. } => true,
        CompositorDisplayCommand::Border(border) => nodes.insert(border.node),
        CompositorDisplayCommand::Rect(rect) => {
            rect.generation != 0 && !rect.geometry.is_empty() && nodes.insert(rect.node)
        }
        CompositorDisplayCommand::Text(text) => {
            text.generation != 0
                && !text.geometry.is_empty()
                && !text.text.is_empty()
                && text.text.len() <= sophia_protocol::MAX_CHROME_LABEL_LEN
                && !text.text.chars().any(char::is_control)
                && text.font_size_millis != 0
                && nodes.insert(text.node)
        }
        CompositorDisplayCommand::IndicatorStrip(strip) => nodes.insert(strip.node),
        CompositorDisplayCommand::ContentImage(image) => {
            image.generation != 0
                && image.output_size_px.width > 0
                && image.output_size_px.height > 0
                && !image.geometry_px.is_empty()
                && image.size_px.width > 0
                && image.size_px.height > 0
                && image.stride
                    == u32::try_from(image.size_px.width)
                        .ok()
                        .and_then(|width| width.checked_mul(4))
                        .unwrap_or(0)
                && image.format == sophia_renderer_live_format_argb8888()
                && image.resource.bytes().len()
                    == usize::try_from(image.stride)
                        .ok()
                        .and_then(|stride| {
                            usize::try_from(image.size_px.height)
                                .ok()
                                .and_then(|height| stride.checked_mul(height))
                        })
                        .unwrap_or(usize::MAX)
                && nodes.insert(image.node)
        }
    })
}

const fn sophia_renderer_live_format_argb8888() -> u32 {
    // DRM_FORMAT_ARGB8888. Kept here to avoid making Engine depend on the
    // renderer crate merely for a wire-format constant.
    u32::from_le_bytes(*b"AR24")
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FocusRingStyle {
    pub width: i32,
    pub color: CompositorRgb8,
}

impl Default for FocusRingStyle {
    fn default() -> Self {
        Self {
            width: 2,
            color: CompositorRgb8 {
                red: 0x70,
                green: 0xb7,
                blue: 0xff,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SurfaceFrameStyle {
    pub width: i32,
    pub focused_color: CompositorRgb8,
    pub unfocused_color: CompositorRgb8,
}

impl Default for SurfaceFrameStyle {
    fn default() -> Self {
        Self {
            width: 0,
            focused_color: FocusRingStyle::default().color,
            unfocused_color: CompositorRgb8 {
                red: 0x30,
                green: 0x30,
                blue: 0x30,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SurfaceChromeStyle {
    pub focus_ring: FocusRingStyle,
    pub frame: SurfaceFrameStyle,
}

impl SurfaceChromeStyle {
    pub const fn clearance(self) -> i32 {
        let ring = if self.focus_ring.width > 0 {
            self.focus_ring.width
        } else {
            0
        };
        let frame = if self.frame.width > 0 {
            self.frame.width
        } else {
            0
        };
        if ring > frame { ring } else { frame }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompositorDisplayListError {
    InvalidOutput,
    InvalidSurface,
    DuplicateSurface,
    CapacityExceeded,
}

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
                let empty = CompositorDisplayList::empty(self.output.id);
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

impl fmt::Display for CompositorDisplayListError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for CompositorDisplayListError {}

/// Builds one immutable compositor display list from committed visual state.
///
/// Surface commands preserve Engine presentation order. Engine-owned frame and
/// focus-ring nodes are inserted before their content surface.
pub fn surface_chrome_display_list(
    output: OutputId,
    presentation_order: &[SurfaceId],
    committed_surfaces: &[CommittedSurfaceState],
    focused_surface: Option<SurfaceId>,
    style: SurfaceChromeStyle,
) -> Result<CompositorDisplayList, CompositorDisplayListError> {
    surface_chrome_display_list_for_surfaces(
        output,
        presentation_order,
        presentation_order,
        committed_surfaces,
        focused_surface,
        style,
    )
}

pub fn surface_chrome_display_list_for_surfaces(
    output: OutputId,
    presentation_order: &[SurfaceId],
    chrome_surfaces: &[SurfaceId],
    committed_surfaces: &[CommittedSurfaceState],
    focused_surface: Option<SurfaceId>,
    style: SurfaceChromeStyle,
) -> Result<CompositorDisplayList, CompositorDisplayListError> {
    if !output.is_valid() {
        return Err(CompositorDisplayListError::InvalidOutput);
    }
    let mut commands = Vec::with_capacity(
        presentation_order
            .len()
            .saturating_mul(2)
            .saturating_add(1)
            .min(MAX_COMPOSITOR_DISPLAY_COMMANDS),
    );
    let mut seen = BTreeSet::new();
    for surface in presentation_order.iter().copied() {
        if !surface.is_valid() {
            return Err(CompositorDisplayListError::InvalidSurface);
        }
        if !seen.insert(surface) {
            return Err(CompositorDisplayListError::DuplicateSurface);
        }
        let Some(committed) = committed_surfaces
            .iter()
            .find(|committed| committed.surface == surface)
        else {
            push_display_command(&mut commands, CompositorDisplayCommand::Surface { surface })?;
            continue;
        };
        let focused = focused_surface == Some(surface);
        if !chrome_surfaces.contains(&surface) {
            push_display_command(&mut commands, CompositorDisplayCommand::Surface { surface })?;
            continue;
        }
        if let Some(frame) = surface_chrome_border(
            committed,
            SurfaceChromeRole::Frame,
            style.frame.width,
            if focused {
                style.frame.focused_color
            } else {
                style.frame.unfocused_color
            },
        ) {
            push_display_command(&mut commands, CompositorDisplayCommand::Border(frame))?;
        }
        if focused
            && let Some(ring) = surface_chrome_border(
                committed,
                SurfaceChromeRole::FocusRing,
                style.focus_ring.width,
                style.focus_ring.color,
            )
        {
            push_display_command(&mut commands, CompositorDisplayCommand::Border(ring))?;
        }
        push_display_command(&mut commands, CompositorDisplayCommand::Surface { surface })?;
    }
    Ok(CompositorDisplayList { output, commands })
}

/// Computes compositor-owned damage between two immutable display lists.
///
/// Stable nodes with an unchanged generation, geometry, and color contribute
/// no damage. Changed and removed nodes damage their old extents; changed and
/// created nodes damage their new extents.
pub fn compositor_display_list_damage(
    previous: &CompositorDisplayList,
    current: &CompositorDisplayList,
) -> Region {
    let previous_borders = previous
        .borders()
        .map(|border| (border.node, border))
        .collect::<BTreeMap<_, _>>();
    let current_borders = current
        .borders()
        .map(|border| (border.node, border))
        .collect::<BTreeMap<_, _>>();
    let mut damage = Region::empty();
    for node in previous_borders
        .keys()
        .chain(current_borders.keys())
        .copied()
        .collect::<BTreeSet<_>>()
    {
        match (previous_borders.get(&node), current_borders.get(&node)) {
            (Some(before), Some(after)) if before == after => {}
            (Some(before), Some(after)) => {
                push_border_damage(&mut damage, *before);
                push_border_damage(&mut damage, *after);
            }
            (Some(before), None) => push_border_damage(&mut damage, *before),
            (None, Some(after)) => push_border_damage(&mut damage, *after),
            (None, None) => unreachable!("node came from one display list"),
        }
    }
    let previous_strips = previous
        .indicator_strips()
        .map(|strip| (strip.node, strip))
        .collect::<BTreeMap<_, _>>();
    let current_strips = current
        .indicator_strips()
        .map(|strip| (strip.node, strip))
        .collect::<BTreeMap<_, _>>();
    for node in previous_strips
        .keys()
        .chain(current_strips.keys())
        .copied()
        .collect::<BTreeSet<_>>()
    {
        match (previous_strips.get(&node), current_strips.get(&node)) {
            (Some(before), Some(after)) if before == after => {}
            (Some(before), Some(after)) => {
                damage.push(before.strip.geometry);
                damage.push(after.strip.geometry);
            }
            (Some(before), None) => damage.push(before.strip.geometry),
            (None, Some(after)) => damage.push(after.strip.geometry),
            (None, None) => unreachable!("node came from one display list"),
        }
    }
    let previous_rects = previous
        .rects()
        .map(|rect| (rect.node, rect))
        .collect::<BTreeMap<_, _>>();
    let current_rects = current
        .rects()
        .map(|rect| (rect.node, rect))
        .collect::<BTreeMap<_, _>>();
    for node in previous_rects
        .keys()
        .chain(current_rects.keys())
        .copied()
        .collect::<BTreeSet<_>>()
    {
        match (previous_rects.get(&node), current_rects.get(&node)) {
            (Some(before), Some(after)) if before == after => {}
            (Some(before), Some(after)) => {
                damage.push(before.geometry);
                damage.push(after.geometry);
            }
            (Some(before), None) => damage.push(before.geometry),
            (None, Some(after)) => damage.push(after.geometry),
            (None, None) => unreachable!("node came from one display list"),
        }
    }
    let previous_texts = previous
        .texts()
        .map(|text| (text.node, text))
        .collect::<BTreeMap<_, _>>();
    let current_texts = current
        .texts()
        .map(|text| (text.node, text))
        .collect::<BTreeMap<_, _>>();
    for node in previous_texts
        .keys()
        .chain(current_texts.keys())
        .copied()
        .collect::<BTreeSet<_>>()
    {
        match (previous_texts.get(&node), current_texts.get(&node)) {
            (Some(before), Some(after)) if before == after => {}
            (Some(before), Some(after)) => {
                damage.push(before.geometry);
                damage.push(after.geometry);
            }
            (Some(before), None) => damage.push(before.geometry),
            (None, Some(after)) => damage.push(after.geometry),
            (None, None) => unreachable!("node came from one display list"),
        }
    }
    let previous_images = previous
        .content_images()
        .map(|image| (image.node, image))
        .collect::<BTreeMap<_, _>>();
    let current_images = current
        .content_images()
        .map(|image| (image.node, image))
        .collect::<BTreeMap<_, _>>();
    for node in previous_images
        .keys()
        .chain(current_images.keys())
        .copied()
        .collect::<BTreeSet<_>>()
    {
        match (previous_images.get(&node), current_images.get(&node)) {
            (Some(before), Some(after)) if before == after => {}
            (Some(before), Some(after)) => {
                damage.push(before.geometry_px);
                damage.push(after.geometry_px);
            }
            (Some(before), None) => damage.push(before.geometry_px),
            (None, Some(after)) => damage.push(after.geometry_px),
            (None, None) => unreachable!("node came from one display list"),
        }
    }
    damage
}

fn push_display_command(
    commands: &mut Vec<CompositorDisplayCommand>,
    command: CompositorDisplayCommand,
) -> Result<(), CompositorDisplayListError> {
    if commands.len() >= MAX_COMPOSITOR_DISPLAY_COMMANDS {
        return Err(CompositorDisplayListError::CapacityExceeded);
    }
    commands.push(command);
    Ok(())
}

fn surface_chrome_border(
    committed: &CommittedSurfaceState,
    role: SurfaceChromeRole,
    width: i32,
    color: CompositorRgb8,
) -> Option<CompositorBorder> {
    let inner = committed.geometry;
    if inner.is_empty() || width <= 0 {
        return None;
    }
    let doubled = width.checked_mul(2)?;
    let outer = Rect {
        x: inner.x.checked_sub(width)?,
        y: inner.y.checked_sub(width)?,
        width: inner.width.checked_add(doubled)?,
        height: inner.height.checked_add(doubled)?,
    };
    Some(CompositorBorder {
        node: CompositorNodeId::SurfaceChrome {
            surface: committed.surface,
            role,
        },
        generation: surface_chrome_generation(inner, width, color, role),
        outer,
        inner,
        color,
    })
}

pub fn compositor_floating_outline(
    surface: SurfaceId,
    geometry: Rect,
    width: i32,
    color: CompositorRgb8,
) -> Option<CompositorBorder> {
    if !surface.is_valid() || geometry.is_empty() || width <= 0 {
        return None;
    }
    let committed = CommittedSurfaceState {
        surface,
        committed_generation: 0,
        geometry,
        content: sophia_protocol::SurfaceContentSet::singleton(
            BufferSource::None,
            sophia_protocol::Size {
                width: geometry.width,
                height: geometry.height,
            },
        ),
        damage: Region::empty(),
    };
    surface_chrome_border(&committed, SurfaceChromeRole::FloatingOutline, width, color)
}

pub fn compositor_border_bands(border: CompositorBorder) -> [CompositorSolidRect; 4] {
    let outer = border.outer;
    let inner = border.inner;
    [
        CompositorSolidRect {
            geometry: Rect {
                height: inner.y.saturating_sub(outer.y),
                ..outer
            },
            color: border.color,
        },
        CompositorSolidRect {
            geometry: Rect {
                y: inner.y.saturating_add(inner.height),
                height: outer
                    .y
                    .saturating_add(outer.height)
                    .saturating_sub(inner.y.saturating_add(inner.height)),
                ..outer
            },
            color: border.color,
        },
        CompositorSolidRect {
            geometry: Rect {
                y: inner.y,
                width: inner.x.saturating_sub(outer.x),
                height: inner.height,
                ..outer
            },
            color: border.color,
        },
        CompositorSolidRect {
            geometry: Rect {
                x: inner.x.saturating_add(inner.width),
                y: inner.y,
                width: outer
                    .x
                    .saturating_add(outer.width)
                    .saturating_sub(inner.x.saturating_add(inner.width)),
                height: inner.height,
            },
            color: border.color,
        },
    ]
}

fn push_border_damage(damage: &mut Region, border: CompositorBorder) {
    for band in compositor_border_bands(border) {
        if !band.geometry.is_empty() {
            damage.push(band.geometry);
        }
    }
}

fn surface_chrome_generation(
    geometry: Rect,
    width: i32,
    color: CompositorRgb8,
    role: SurfaceChromeRole,
) -> u64 {
    let mut generation = 0xcbf2_9ce4_8422_2325u64;
    for byte in geometry
        .x
        .to_le_bytes()
        .into_iter()
        .chain(geometry.y.to_le_bytes())
        .chain(geometry.width.to_le_bytes())
        .chain(geometry.height.to_le_bytes())
        .chain(width.to_le_bytes())
        .chain([color.red, color.green, color.blue, role as u8])
    {
        generation = (generation ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3);
    }
    generation.max(1)
}
