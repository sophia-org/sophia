use crate::LiveProductionNativeFrameId;
use sophia_engine::RenderHeadId;
use sophia_protocol::{OutputId, TransactionId};
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

pub const LIVE_PRODUCTION_PAGE_FLIP_HARD_STALL: Duration = Duration::from_millis(500);

/// Settles an initialization transaction after its fallible KMS work.
///
/// Once at least one physical head exists, a failed initialization may already
/// have installed scanout resources on an earlier head. The caller must therefore
/// run its ownership rollback before returning the initialization error. Keeping
/// this decision in one helper also makes the error-preservation contract
/// independently testable without opening real DRM devices.
pub fn finish_live_production_native_initialization(
    initialized: Result<(), Box<dyn std::error::Error>>,
    rollback_required: bool,
    rollback: impl FnOnce() -> Result<(), Box<dyn std::error::Error>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let Err(error) = initialized else {
        return Ok(());
    };
    if !rollback_required {
        return Err(error);
    }
    match rollback() {
        Ok(()) => Err(error),
        Err(rollback) => Err(format!(
            "native output initialization failed: {error}; rollback failed: {rollback}"
        )
        .into()),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveProductionMirrorGroupBegin {
    Started,
    GenerationInFlight,
    Poisoned,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveProductionMirrorHeadTransition {
    Accepted,
    GroupReady,
    Duplicate,
    UnknownHead,
    WrongGeneration,
    NotSubmitted,
}

/// Paces the physical heads of one logical mirror output independently.
///
/// `newest` is the latest generation with complete per-head renderer input.
/// The Engine cohort separately enforces prepare-all before the first submit.
/// Each idle head may then submit `newest` without consulting a sibling's KMS
/// state, and a lagging head skips directly to it. The primary head alone
/// produces logical presentation; native owners remain head-scoped outside
/// this passive reducer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveProductionMirrorGroupLifecycle {
    output: OutputId,
    primary: RenderHeadId,
    required: BTreeSet<RenderHeadId>,
    initialized: BTreeSet<RenderHeadId>,
    newest: Option<LiveProductionNativeFrameId>,
    active_progress_at: Option<std::time::Instant>,
    inflight: BTreeMap<RenderHeadId, LiveProductionNativeFrameId>,
    displayed: BTreeMap<RenderHeadId, LiveProductionNativeFrameId>,
    failed: bool,
    completed: Option<LiveProductionNativeFrameId>,
    completed_ust_usec: Option<u64>,
}

impl LiveProductionMirrorGroupLifecycle {
    pub fn new(output: OutputId, heads: impl IntoIterator<Item = RenderHeadId>) -> Option<Self> {
        let ordered = heads.into_iter().collect::<Vec<_>>();
        let primary = *ordered.first()?;
        let required = ordered.into_iter().collect::<BTreeSet<_>>();
        (!required.is_empty() && required.iter().all(|head| head.is_valid())).then_some(Self {
            output,
            primary,
            required,
            initialized: BTreeSet::new(),
            newest: None,
            active_progress_at: None,
            inflight: BTreeMap::new(),
            displayed: BTreeMap::new(),
            failed: false,
            completed: None,
            completed_ust_usec: None,
        })
    }

    pub const fn output(&self) -> OutputId {
        self.output
    }

    pub fn heads(&self) -> impl Iterator<Item = RenderHeadId> + '_ {
        self.required.iter().copied()
    }

    pub const fn primary_head(&self) -> RenderHeadId {
        self.primary
    }

    pub fn initialized(&self) -> bool {
        self.initialized == self.required
    }

    pub fn mark_initialized(&mut self, head: RenderHeadId) -> LiveProductionMirrorHeadTransition {
        if !self.required.contains(&head) {
            return LiveProductionMirrorHeadTransition::UnknownHead;
        }
        if !self.initialized.insert(head) {
            return LiveProductionMirrorHeadTransition::Duplicate;
        }
        if self.initialized() {
            LiveProductionMirrorHeadTransition::GroupReady
        } else {
            LiveProductionMirrorHeadTransition::Accepted
        }
    }

    pub fn begin(&mut self, frame: LiveProductionNativeFrameId) -> LiveProductionMirrorGroupBegin {
        if self.failed {
            return LiveProductionMirrorGroupBegin::Poisoned;
        }
        if self.newest.is_some_and(|newest| frame <= newest) {
            return LiveProductionMirrorGroupBegin::GenerationInFlight;
        }
        self.newest = Some(frame);
        self.active_progress_at = Some(std::time::Instant::now());
        LiveProductionMirrorGroupBegin::Started
    }

    pub const fn active_frame(&self) -> Option<LiveProductionNativeFrameId> {
        self.newest
    }

    /// Age since the reserved generation last made physical progress.
    /// This starts at queue reservation and resets when a renderer worker starts,
    /// a head submits, or a head flips, so a head that never reaches
    /// `submitted_at` cannot evade the group watchdog.
    pub fn active_age(&self) -> Option<Duration> {
        (!self.converged())
            .then(|| self.active_progress_at.map(|progress| progress.elapsed()))
            .flatten()
    }

    pub fn active_generation_hard_stalled(&self, hard_stall: Duration) -> bool {
        self.active_age().is_some_and(|age| age >= hard_stall)
    }

    pub fn observe_physical_progress(&mut self, frame: LiveProductionNativeFrameId) -> bool {
        if self.newest != Some(frame) {
            return false;
        }
        self.active_progress_at = Some(std::time::Instant::now());
        true
    }

    pub const fn failed(&self) -> bool {
        self.failed
    }

    pub fn awaiting_flips(&self) -> bool {
        !self.inflight.is_empty()
    }

    /// Logical identity currently submitted on the primary head.
    pub fn logically_submitted_frame(&self) -> Option<LiveProductionNativeFrameId> {
        self.inflight.get(&self.primary).copied()
    }

    /// Whether this head may consume renderer work for the current turn.
    ///
    /// A physical callback clears this head's KMS submission independently of
    /// its siblings. The per-head in-flight map is therefore the generation
    /// fence.
    pub fn head_may_submit(&self, head: RenderHeadId) -> bool {
        !self.failed && self.required.contains(&head) && !self.inflight.contains_key(&head)
    }

    /// Whether this head may submit this exact logical generation.
    ///
    /// The head fence prevents a fast head from submitting twice, while the
    /// frame fence prevents renderer work queued for the successor from being
    /// relabeled as the still-active generation.
    pub fn head_may_submit_frame(
        &self,
        head: RenderHeadId,
        frame: LiveProductionNativeFrameId,
    ) -> bool {
        self.head_may_submit(head)
            && self.newest == Some(frame)
            && self
                .displayed
                .get(&head)
                .is_none_or(|displayed| *displayed < frame)
    }

    /// Poisons a partially submitted generation.
    ///
    /// Already accepted physical commits still own their resources and must
    /// drain, but no later tick may submit another logical generation or publish
    /// this one as presented.
    pub fn abort(&mut self, frame: LiveProductionNativeFrameId) -> bool {
        if self.newest != Some(frame) && !self.inflight.values().any(|active| *active == frame) {
            return false;
        }
        self.failed = true;
        true
    }

    pub fn mark_submitted(
        &mut self,
        head: RenderHeadId,
        frame: LiveProductionNativeFrameId,
    ) -> LiveProductionMirrorHeadTransition {
        if !self.required.contains(&head) {
            return LiveProductionMirrorHeadTransition::UnknownHead;
        }
        if self.newest != Some(frame) {
            return LiveProductionMirrorHeadTransition::WrongGeneration;
        }
        if !self.head_may_submit_frame(head, frame) {
            return LiveProductionMirrorHeadTransition::Duplicate;
        }
        self.inflight.insert(head, frame);
        self.active_progress_at = Some(std::time::Instant::now());
        if head == self.primary {
            LiveProductionMirrorHeadTransition::GroupReady
        } else {
            LiveProductionMirrorHeadTransition::Accepted
        }
    }

    pub fn mark_flipped(
        &mut self,
        head: RenderHeadId,
        frame: LiveProductionNativeFrameId,
    ) -> LiveProductionMirrorHeadTransition {
        if !self.required.contains(&head) {
            return LiveProductionMirrorHeadTransition::UnknownHead;
        }
        match self.inflight.get(&head) {
            None => return LiveProductionMirrorHeadTransition::NotSubmitted,
            Some(submitted) if *submitted != frame => {
                return LiveProductionMirrorHeadTransition::WrongGeneration;
            }
            Some(_) => {}
        }
        self.inflight.remove(&head);
        self.displayed.insert(head, frame);
        self.active_progress_at = Some(std::time::Instant::now());
        if head == self.primary {
            self.completed = Some(frame);
            LiveProductionMirrorHeadTransition::GroupReady
        } else {
            LiveProductionMirrorHeadTransition::Accepted
        }
    }

    /// Records physical timing for the active generation.
    ///
    /// Logical feedback uses the latest physical UST and the logical generation
    /// as MSC. Kernel sequences belong to individual CRTCs and cannot form one
    /// coherent output-wide sequence.
    pub fn observe_flip_timing(
        &mut self,
        head: RenderHeadId,
        frame: LiveProductionNativeFrameId,
        _serial: u64,
        ust_usec: u64,
    ) -> bool {
        if self.inflight.get(&head) != Some(&frame) {
            return false;
        }
        if head == self.primary {
            self.completed_ust_usec = Some(ust_usec);
        }
        true
    }

    pub const fn flip_timing(&self) -> Option<(u64, u64)> {
        match (self.completed, self.completed_ust_usec) {
            (Some(frame), Some(ust_usec)) => Some((frame.raw(), ust_usec)),
            _ => None,
        }
    }

    pub const fn completed_frame(&self) -> Option<LiveProductionNativeFrameId> {
        self.completed
    }

    pub fn take_completed_frame(&mut self) -> Option<LiveProductionNativeFrameId> {
        let completed = self.completed.take();
        if completed.is_some() {
            self.completed_ust_usec = None;
        }
        completed
    }

    pub fn displayed_frame(&self, head: RenderHeadId) -> Option<LiveProductionNativeFrameId> {
        self.displayed.get(&head).copied()
    }

    pub fn generation_is_scanned(&self, frame: LiveProductionNativeFrameId) -> bool {
        self.inflight
            .values()
            .chain(self.displayed.values())
            .any(|owned| *owned == frame)
    }

    pub fn converged(&self) -> bool {
        self.newest.is_some_and(|newest| {
            self.required
                .iter()
                .all(|head| self.displayed.get(head) == Some(&newest))
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveProductionPageFlipWatchdogStatus {
    Idle,
    Healthy,
    HardStall,
}

pub fn reduce_live_production_page_flip_watchdog(
    submitted_age: Option<Duration>,
    hard_stall: Duration,
) -> LiveProductionPageFlipWatchdogStatus {
    match submitted_age {
        None => LiveProductionPageFlipWatchdogStatus::Idle,
        Some(age) if age >= hard_stall => LiveProductionPageFlipWatchdogStatus::HardStall,
        Some(_) => LiveProductionPageFlipWatchdogStatus::Healthy,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiveProductionScanoutContent {
    Cpu {
        frame: LiveProductionNativeFrameId,
        checksum: u64,
    },
    MixedPresent {
        frame: LiveProductionNativeFrameId,
        transaction: TransactionId,
        nonzero_rgb_pixels: usize,
    },
    RetainedMixed {
        frame: LiveProductionNativeFrameId,
        nonzero_rgb_pixels: usize,
    },
    HeadComposition {
        frame: LiveProductionNativeFrameId,
        logical_content_checksum: u64,
        nonzero_rgb_pixels: usize,
    },
}

impl LiveProductionScanoutContent {
    /// The logical scene this content draws, where the content names one.
    ///
    /// Present-backed variants carry client pixels rather than a composed scene,
    /// so they report none and are never treated as interchangeable.
    pub const fn logical_checksum(self) -> Option<u64> {
        match self {
            Self::Cpu { checksum, .. } => Some(checksum),
            Self::HeadComposition {
                logical_content_checksum,
                ..
            } => Some(logical_content_checksum),
            Self::MixedPresent { .. } | Self::RetainedMixed { .. } => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveProductionRetainedSceneQueueStatus {
    Queue,
    UnchangedPending,
    UnchangedRendering,
    UnchangedSubmitted,
    UnchangedPresented,
}

/// Why a retained composition is being queued.
///
/// Ordinary projections only need the newest logical pixels on glass and may
/// reuse identical work already owned by the output. Presentation feedback is
/// different: every accepted Present needs a distinct physical retirement,
/// even when its pixels have the same checksum as the displayed scene.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveProductionRetainedFrameQueueRequirement {
    LatestScene,
    FreshRetirement,
}

pub fn reduce_live_production_retained_frame_queue(
    requirement: LiveProductionRetainedFrameQueueRequirement,
    pending: Option<LiveProductionScanoutContent>,
    rendering: Option<LiveProductionScanoutContent>,
    submitted: Option<LiveProductionScanoutContent>,
    presented: Option<LiveProductionScanoutContent>,
    checksum: u64,
) -> LiveProductionRetainedSceneQueueStatus {
    if requirement == LiveProductionRetainedFrameQueueRequirement::FreshRetirement {
        return LiveProductionRetainedSceneQueueStatus::Queue;
    }
    reduce_live_production_retained_scene_queue(pending, rendering, submitted, presented, checksum)
}

/// Reduces retained-scene queueing against the newest frame a head owns.
///
/// Ownership order matters. If a different pending frame is newer than a
/// matching displayed frame, the requested checksum is a real change back and
/// must queue. Otherwise an identical rendering, submitted, or displayed scene
/// is already sufficient and another KMS flip would carry no new pixels.
pub fn reduce_live_production_retained_scene_queue(
    pending: Option<LiveProductionScanoutContent>,
    rendering: Option<LiveProductionScanoutContent>,
    submitted: Option<LiveProductionScanoutContent>,
    presented: Option<LiveProductionScanoutContent>,
    checksum: u64,
) -> LiveProductionRetainedSceneQueueStatus {
    for (content, unchanged) in [
        (
            pending,
            LiveProductionRetainedSceneQueueStatus::UnchangedPending,
        ),
        (
            rendering,
            LiveProductionRetainedSceneQueueStatus::UnchangedRendering,
        ),
        (
            submitted,
            LiveProductionRetainedSceneQueueStatus::UnchangedSubmitted,
        ),
        (
            presented,
            LiveProductionRetainedSceneQueueStatus::UnchangedPresented,
        ),
    ] {
        let Some(content) = content else {
            continue;
        };
        return if content.logical_checksum() == Some(checksum) {
            unchanged
        } else {
            LiveProductionRetainedSceneQueueStatus::Queue
        };
    }
    LiveProductionRetainedSceneQueueStatus::Queue
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveProductionMirrorGenerationQueue {
    Install,
    DeferUntilPrimarySubmission,
}

/// Keeps an unsubmitted Present generation from being silently coalesced.
///
/// Once the primary owns the generation, a successor may advance normally:
/// the primary will produce logical presentation and any lagging secondary can
/// skip directly to the successor. Before that point, replacing the generation
/// would leave the Present scheduler waiting for pixels no head can submit.
pub fn reduce_live_production_mirror_generation_queue(
    active_frame: Option<LiveProductionNativeFrameId>,
    primary_owned_frame: Option<LiveProductionNativeFrameId>,
    active_content: Option<LiveProductionScanoutContent>,
) -> LiveProductionMirrorGenerationQueue {
    match (active_frame, active_content) {
        (Some(active), Some(LiveProductionScanoutContent::MixedPresent { frame, .. }))
            if frame == active && primary_owned_frame != Some(active) =>
        {
            LiveProductionMirrorGenerationQueue::DeferUntilPrimarySubmission
        }
        _ => LiveProductionMirrorGenerationQueue::Install,
    }
}

impl LiveProductionScanoutContent {
    pub const fn frame(self) -> LiveProductionNativeFrameId {
        match self {
            Self::Cpu { frame, .. }
            | Self::MixedPresent { frame, .. }
            | Self::RetainedMixed { frame, .. }
            | Self::HeadComposition { frame, .. } => frame,
        }
    }

    pub const fn with_nonzero_rgb_pixels(self, nonzero_rgb_pixels: usize) -> Self {
        match self {
            Self::MixedPresent {
                frame, transaction, ..
            } => Self::MixedPresent {
                frame,
                transaction,
                nonzero_rgb_pixels,
            },
            Self::RetainedMixed { frame, .. } => Self::RetainedMixed {
                frame,
                nonzero_rgb_pixels,
            },
            Self::HeadComposition {
                frame,
                logical_content_checksum,
                ..
            } => Self::HeadComposition {
                frame,
                logical_content_checksum,
                nonzero_rgb_pixels,
            },
            cpu @ Self::Cpu { .. } => cpu,
        }
    }

    pub const fn cpu_checksum(self) -> Option<u64> {
        match self {
            Self::Cpu { checksum, .. } => Some(checksum),
            Self::HeadComposition {
                logical_content_checksum,
                ..
            } => Some(logical_content_checksum),
            Self::MixedPresent { .. } | Self::RetainedMixed { .. } => None,
        }
    }

    pub const fn source_label(self) -> &'static str {
        match self {
            Self::Cpu { .. } => "cpu",
            Self::MixedPresent { .. } => "mixed_present",
            Self::RetainedMixed { .. } => "retained_mixed",
            Self::HeadComposition { .. } => "head_composition",
        }
    }

    pub fn same_logical_identity(self, other: Self) -> bool {
        match (self, other) {
            (
                Self::Cpu { frame, checksum },
                Self::Cpu {
                    frame: other_frame,
                    checksum: other_checksum,
                },
            ) => frame == other_frame && checksum == other_checksum,
            (
                Self::MixedPresent {
                    frame, transaction, ..
                },
                Self::MixedPresent {
                    frame: other_frame,
                    transaction: other_transaction,
                    ..
                },
            ) => frame == other_frame && transaction == other_transaction,
            (
                Self::RetainedMixed { frame, .. },
                Self::RetainedMixed {
                    frame: other_frame, ..
                },
            ) => frame == other_frame,
            (
                Self::HeadComposition {
                    frame,
                    logical_content_checksum,
                    ..
                },
                Self::HeadComposition {
                    frame: other_frame,
                    logical_content_checksum: other_checksum,
                    ..
                },
            ) => frame == other_frame && logical_content_checksum == other_checksum,
            _ => false,
        }
    }
}

/// Identifies the renderer work that the next exporter poll will complete.
///
/// An in-flight worker owns `rendering_content`; `pending_content` can already
/// contain the next generation and must not be used to identify that result.
pub fn live_production_mirror_head_work_frame(
    worker_in_flight: bool,
    rendering_content: Option<LiveProductionScanoutContent>,
    pending_content: Option<LiveProductionScanoutContent>,
) -> Option<LiveProductionNativeFrameId> {
    if worker_in_flight {
        rendering_content
    } else {
        pending_content
    }
    .map(LiveProductionScanoutContent::frame)
}

/// Moves the exact queued identity into renderer ownership when a worker starts.
///
/// The worker transition is authoritative even when the surrounding backend tick
/// has no submit report. Tying this move to `ScanoutExportPending` left the
/// renderer holding pixels while `pending_content` still named them; the next
/// tick then tried to retire an empty `rendering_content` slot.
pub fn advance_live_production_renderer_content(
    worker_was_in_flight: bool,
    worker_is_in_flight: bool,
    pending_content: &mut Option<LiveProductionScanoutContent>,
    rendering_content: &mut Option<LiveProductionScanoutContent>,
) -> Result<bool, &'static str> {
    if worker_was_in_flight || !worker_is_in_flight {
        return Ok(false);
    }
    if rendering_content.is_some() {
        return Err("renderer worker started while another content identity was rendering");
    }
    let content = pending_content
        .take()
        .ok_or("renderer worker started without a pending content identity")?;
    *rendering_content = Some(content);
    Ok(true)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LiveProductionNativeFrameRetirement {
    pub output: OutputId,
    pub frame: LiveProductionNativeFrameId,
    pub submission: u64,
    pub content: LiveProductionScanoutContent,
    /// Whether the retiring frame put the client's own buffer on the plane.
    ///
    /// A direct frame settles as `Flip` and keeps its buffer owed to the
    /// client until a successor retires it; a composed one settles as `Copied`
    /// and releases at the flip. See `PresentFlipOwnership.tla`.
    pub direct: bool,
    pub layout_witness: Option<super::LiveProductionRetiredLayoutWitness>,
    pub ust: u64,
    pub msc: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveProductionCpuFrameQueueStatus {
    Queued,
    /// The named logical output has no head to queue against. Distinct from every
    /// other status here, which describe a head declining the frame: this one says
    /// the frame was addressed to something that is not a screen.
    NoHead,
    BaselineRequired,
    GpuFrameOwned,
    UnchangedPending,
    UnchangedSubmitted,
    UnchangedPresented,
}

pub fn reduce_live_production_cpu_frame_queue(
    pending: Option<LiveProductionScanoutContent>,
    submitted: Option<LiveProductionScanoutContent>,
    presented: Option<LiveProductionScanoutContent>,
    renderer_in_flight: bool,
    callback_observed: bool,
    checksum: u64,
) -> LiveProductionCpuFrameQueueStatus {
    if renderer_in_flight
        || matches!(
            pending,
            Some(
                LiveProductionScanoutContent::MixedPresent { .. }
                    | LiveProductionScanoutContent::RetainedMixed { .. }
                    | LiveProductionScanoutContent::HeadComposition { .. }
            )
        )
        || matches!(
            submitted,
            Some(
                LiveProductionScanoutContent::MixedPresent { .. }
                    | LiveProductionScanoutContent::RetainedMixed { .. }
                    | LiveProductionScanoutContent::HeadComposition { .. }
            )
        )
    {
        LiveProductionCpuFrameQueueStatus::GpuFrameOwned
    } else if matches!(pending, Some(LiveProductionScanoutContent::Cpu {
        checksum: pending_checksum,
        ..
    }) if pending_checksum == checksum)
    {
        LiveProductionCpuFrameQueueStatus::UnchangedPending
    } else if matches!(submitted, Some(LiveProductionScanoutContent::Cpu {
        checksum: submitted_checksum,
        ..
    }) if submitted_checksum == checksum)
    {
        LiveProductionCpuFrameQueueStatus::UnchangedSubmitted
    } else if pending.is_none()
        && submitted.is_none()
        && matches!(presented, Some(LiveProductionScanoutContent::Cpu {
            checksum: presented_checksum,
            ..
        }) if presented_checksum == checksum)
        && !callback_observed
    {
        LiveProductionCpuFrameQueueStatus::BaselineRequired
    } else if pending.is_none()
        && submitted.is_none()
        && matches!(presented, Some(LiveProductionScanoutContent::Cpu {
            checksum: presented_checksum,
            ..
        }) if presented_checksum == checksum)
    {
        LiveProductionCpuFrameQueueStatus::UnchangedPresented
    } else {
        LiveProductionCpuFrameQueueStatus::Queued
    }
}

/// Whether this Present's pixels reached the screen.
///
/// The question is about one transaction's own page flip, so it asks only
/// about what is displayed. Work that arrived afterwards -- queued, prepared,
/// or already submitted -- does not unsettle a frame the kernel has shown.
///
/// It used to also demand that nothing newer was submitted and that no head
/// was busy, which asked for a globally quiescent instant. Judgement happens
/// after a whole frame-service pass, and that pass polls the retirement and
/// then submits the successor, so the second condition was routinely false by
/// the time it was read. A mirror group made it worse: retirement itself
/// promotes the coalesced successor into every head's exporter slot, so the
/// third condition was falsified during the very retirement being judged. A
/// physical run judged all eleven of its retirements superseded and never
/// reported startup readiness. Those conjuncts looked free when they were
/// written, because the frame reducer then reserved the primary while a
/// present was queued -- but that reservation was the arbitration deadlock,
/// not a source of quiet.
///
/// `PresentFrameOwnership.tla` already draws this line: it permits a successor
/// submitted after the Present's retirement is observed, requiring only that
/// the successor cannot steal or block the captured retirement.
pub fn live_production_scanout_is_stable_present(
    presented: Option<LiveProductionScanoutContent>,
    transaction: TransactionId,
) -> bool {
    matches!(
        presented,
        Some(LiveProductionScanoutContent::MixedPresent {
            transaction: presented_transaction,
            nonzero_rgb_pixels,
            ..
        }) if presented_transaction == transaction && nonzero_rgb_pixels > 0
    )
}
