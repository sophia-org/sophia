use crate::*;
use sophia_engine::*;
use sophia_protocol::*;
use sophia_renderer_live::*;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::time::{Duration, Instant};

/// What one CPU cycle produced: the submission, the surfaces it committed,
/// and how far the cycle got.
type CpuCycleOutcome = (
    LiveProductionCpuCycleSubmission<crate::LiveBackendRuntimeTickReport>,
    Vec<CommittedSurfaceState>,
    LiveProductionCpuProgress,
);

mod authority;
mod composition_target;
mod compositor_graphics;
#[cfg(test)]
#[path = "../tests/support/lifecycle_tests.rs"]
mod lifecycle_tests;
mod ordinary_repaint;
mod output_composition;
use composition_target::NativeCompositionTarget;
mod native;
mod ownership;
mod present;
mod projection;
mod service;
mod software_present;
mod translation;
pub use compositor_graphics::{
    live_present_head_composition_sources, live_surface_routes_to_output,
    live_surfaces_owned_by_output,
};
pub use native::*;
pub use ownership::*;
pub use present::live_present_head_frames_capture_image;
pub use service::*;

fn trace_live_head_composition_plan(plan: &sophia_engine::HeadCompositionPlan) {
    let exact = plan
        .layers
        .iter()
        .filter(|layer| layer.requested_sampling == sophia_engine::HeadSamplingClass::Exact)
        .count();
    let downsampled = plan
        .layers
        .iter()
        .filter(|layer| layer.requested_sampling == sophia_engine::HeadSamplingClass::Downsampled)
        .count();
    let upsampled = plan
        .layers
        .iter()
        .filter(|layer| layer.requested_sampling == sophia_engine::HeadSamplingClass::Upsampled)
        .count();
    let mixed = plan
        .layers
        .iter()
        .filter(|layer| layer.requested_sampling == sophia_engine::HeadSamplingClass::Mixed)
        .count();
    let active = plan
        .layers
        .iter()
        .filter(|layer| layer.outcome == sophia_engine::HeadBindingOutcome::Active)
        .count();
    let fallback = plan.layers.len().saturating_sub(active);
    tracing::trace!(
        "sophia_live_head_composition_plan schema=2 status=ready output={} head={} scene_generation={} target_generation={} width={} height={} mapping={} exact={} downsampled={} upsampled={} mixed={} active={} fallback={} unavailable=0 compositor_primitives={} damage_rects={} logical_content_checksum={}",
        plan.output.raw(),
        plan.head.raw(),
        plan.scene_generation,
        plan.target_generation,
        plan.native_size.width,
        plan.native_size.height,
        plan.mapping.reduced_name(),
        exact,
        downsampled,
        upsampled,
        mixed,
        active,
        fallback,
        plan.compositor.len(),
        plan.repaint.rects.len(),
        plan.logical_content_checksum,
    );
    // Window chrome had no evidence of its own, so a border in the wrong place
    // could only be inferred from the solid rects it eventually became -- and
    // those are traced by the renderer, which is blind to head identity and
    // reports a rect that two heads of the same size both produce. Three
    // diagnoses in a row stalled on exactly that. This states the geometry the
    // plan asked for, on the side that knows which head asked.
    //
    // Both extents, separately: what the chrome spans and what it is allowed to
    // paint into. A band that vanished because it fell outside its scene and one
    // that was never generated look identical downstream.
    for command in &plan.compositor {
        if let sophia_engine::HeadCompositorCommand::Border(border) = command {
            tracing::trace!(
                "sophia_live_head_border schema=1 status=planned output={} head={} scene_generation={} native={}x{} scene={}x{}_{}_{} outer={}x{}_{}_{} inner={}x{}_{}_{} clip={}x{}_{}_{}",
                plan.output.raw(),
                plan.head.raw(),
                plan.scene_generation,
                plan.native_size.width,
                plan.native_size.height,
                plan.transform.projected_scene.width,
                plan.transform.projected_scene.height,
                plan.transform.projected_scene.x,
                plan.transform.projected_scene.y,
                border.outer.width,
                border.outer.height,
                border.outer.x,
                border.outer.y,
                border.inner.width,
                border.inner.height,
                border.inner.x,
                border.inner.y,
                border.clip.width,
                border.clip.height,
                border.clip.x,
                border.clip.y,
            );
        }
    }
    let pixel_trace = std::env::var_os("SOPHIA_NATIVE_COMPOSITION_PIXEL_TRACE").is_some();
    for layer in &plan.layers {
        if pixel_trace {
            let source = match layer.source {
                BufferSource::CpuBuffer { .. } => "cpu",
                BufferSource::DmaBuf { .. } => "dmabuf",
                _ => "other",
            };
            let target = layer.native_geometry;
            let clip = layer.native_clip;
            tracing::info!(
                "sophia_live_head_content_geometry schema=1 status=selected output={} head={} scene_generation={} surface={} committed_generation={} source={source} size={}x{} target={}x{}_{}_{} clip={}x{}_{}_{}",
                plan.output.raw(),
                plan.head.raw(),
                plan.scene_generation,
                layer.surface.index(),
                layer.committed_generation,
                layer.source_pixel_size.width,
                layer.source_pixel_size.height,
                target.width,
                target.height,
                target.x,
                target.y,
                clip.width,
                clip.height,
                clip.x,
                clip.y,
            );
        }
        if let BufferSource::CpuBuffer { handle } = layer.source {
            tracing::trace!(
                "sophia_live_head_content schema=1 status=selected output={} head={} scene_generation={} surface={} committed_generation={} variant={} source=cpu handle={} density_millis={} sampling={} fidelity={}",
                plan.output.raw(),
                plan.head.raw(),
                plan.scene_generation,
                layer.surface.index(),
                layer.committed_generation,
                layer.variant,
                handle,
                layer.density_millis,
                match layer.requested_sampling {
                    sophia_engine::HeadSamplingClass::Exact => "exact",
                    sophia_engine::HeadSamplingClass::Downsampled => "downsampled",
                    sophia_engine::HeadSamplingClass::Upsampled => "upsampled",
                    sophia_engine::HeadSamplingClass::Mixed => "mixed",
                },
                match layer.outcome {
                    sophia_engine::HeadBindingOutcome::Active => "authority_raster",
                    sophia_engine::HeadBindingOutcome::Fallback => "sampled_fallback",
                },
            );
        }
    }
}

#[derive(Debug)]
struct LiveDisplayedSurface {
    layer: LiveRetainedRendererImageLayer,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LiveFocusRingObservation {
    pub surface: SurfaceId,
    pub generation: u64,
    pub primitives: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LiveChromeSetObservation {
    pub generation: u64,
    pub eligible_surfaces: usize,
    pub frames: usize,
    pub focused_frames: usize,
    pub unfocused_frames: usize,
    pub focus_rings: usize,
    pub primitives: usize,
    pub clearance: i32,
}

/// Which production path composed the chrome an observation belongs to.
///
/// The Present turn, the CPU production turn and the cadence repaint each
/// build a display list. Naming the path is what lets a reader tell "two
/// turns disagree about one frame" from "one frame moved".
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveChromeObservationSource {
    Present,
    Production,
    Repaint,
}

impl LiveChromeObservationSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Present => "present",
            Self::Production => "production",
            Self::Repaint => "repaint",
        }
    }
}

/// One framed surface as the chrome set that produced `generation` drew it.
///
/// The chrome-set record carries only a hash, so two alternating generations
/// name nothing. These companions name the frame and the geometry it hugged.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LiveChromeFrameObservation {
    pub generation: u64,
    pub source: LiveChromeObservationSource,
    /// Whether a Present owned the scanout when this chrome was composed.
    pub in_flight: bool,
    pub surface: SurfaceId,
    /// The frame's inner rectangle: the surface geometry the border hugs.
    pub geometry: Rect,
    pub focused: bool,
}

/// Companions kept between authority turns. The printer drains them once a
/// turn; a flood beyond this is dropped, and the set record still names it.
pub(crate) const CHROME_FRAME_OBSERVATION_CAPACITY: usize = 64;
/// Every chrome-set change up to this many is spelled out per frame; later
/// ones only on powers of two, the pacing the Present defer report uses.
pub(crate) const CHROME_SET_CHANGE_DETAIL_LIMIT: u64 = 16;
/// Discarded retirements kept for the next native service report.
pub(crate) const DISCARDED_PRESENT_CAPACITY: usize = 32;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LiveFloatingOutline {
    pub surface: SurfaceId,
    pub geometry: Rect,
}

/// Immutable interaction projection paired with one independently presented
/// output. Its epoch advances only when hit-test meaning changes; buffer-only
/// presentation may replace pixels without invalidating an application lease.
#[derive(Clone, Debug, PartialEq)]
pub struct LivePresentedInputProjection {
    pub output: OutputId,
    pub epoch: u64,
    pub layers: Vec<LayerSnapshot>,
    pub chrome_targets: Vec<sophia_engine::IndicatorChromeHitTarget>,
    pub chrome_occlusion: Option<Rect>,
    pub descriptor_targets: Vec<sophia_engine::PresentedChromeTarget>,
    pub descriptor_occlusion: Option<Rect>,
    pub descriptor_projection: Option<u64>,
    pub tab_occlusions: Vec<Rect>,
    /// Presented components in back-to-front order, with separate grant authority.
    pub content: Vec<sophia_engine::PresentedContentBinding>,
}

/// Session-assigned stacking role, never derived from a client epoch or XID.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum LiveShellContentLayer {
    Shell,
    Dock,
    Launcher,
}

/// Receipt for an exact component removal queued after its pixels were
/// presented. This is not proof of resource release or worker/KMS cleanup.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LiveShellContentRemoval {
    output: OutputId,
    grant: sophia_protocol::ContentGrant,
    candidate: u64,
    prior_presentation_epoch: u64,
}

type ShellContentKey = (OutputId, LiveShellContentLayer);

/// Geometry is captured with admission, never reconstructed from a later layout.
/// The pair moves and rolls back as one owner when native queueing refuses.
#[derive(Clone, Debug, PartialEq)]
struct AdmittedShellContent {
    frame: LiveShellContentFrame,
    interaction_revoked: bool,
    transform: sophia_engine::PresentedContentTransform,
}

/// Retains policy order only for surfaces present in Engine's committed scene.
/// Policy may name a newly admitted surface before matching pixels commit; it
/// is absent from native composition until the ordinary visual commit lands.
pub fn live_production_retained_surface_order(
    presentation_order: &[SurfaceId],
    committed: &[CommittedSurfaceState],
) -> Vec<SurfaceId> {
    let committed = committed
        .iter()
        .map(|state| state.surface)
        .collect::<BTreeSet<_>>();
    presentation_order
        .iter()
        .copied()
        .filter(|surface| committed.contains(surface))
        .collect()
}

fn replace_displayed_surface(
    displayed_surfaces: &mut BTreeMap<SurfaceId, LiveDisplayedSurface>,
    surface: SurfaceId,
    layer: LiveRetainedRendererImageLayer,
) -> Option<LiveDisplayedSurface> {
    displayed_surfaces.insert(surface, LiveDisplayedSurface { layer })
}

/// One Engine-validated content candidate retained until a successor native
/// frame retires it. Each image carries the protocol resource lease that owns
/// its immutable pixels.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveShellContentFrame {
    pub output: OutputId,
    pub content_output: sophia_protocol::ContentOutputId,
    pub grant: sophia_protocol::ContentGrant,
    pub candidate_generation: u64,
    pub interaction_generation: u64,
    pub images: Vec<sophia_engine::CompositorContentImage>,
    pub targets: Vec<sophia_engine::PresentedContentTarget>,
    pub popouts: Vec<sophia_engine::PresentedContentPopout>,
    pub allocations: Vec<(
        sophia_protocol::ContentAllocationId,
        sophia_protocol::ContentLogicalRect,
        sophia_protocol::ContentPixelRect,
    )>,
}

pub struct LiveProductionVisualRuntime {
    /// A revoked native seat must not acquire headless presentation semantics
    /// while final authority removals are drained.
    native_suspended: bool,
    production: sophia_engine::ProductionSessionCoordinator,
    outputs: LiveProductionOutputRuntimeSet,
    surface_metadata: BTreeMap<SurfaceId, projection::LiveSurfaceProjectionMetadata>,
    input_projections: Vec<LivePresentedInputProjection>,
    presentation_feedback: crate::LiveProductionPresentFeedbackCoordinator,
    present_scheduler: LiveProductionPresentScheduler,
    surface_content_stream: SurfaceContentStream<LiveProductionAuthorityGroup>,
    released_surface_content: VecDeque<LiveProductionAuthorityGroup>,
    superseded_surface_content: VecDeque<LiveProductionAuthorityGroup>,
    deferred_content_dma_buf_releases: BTreeSet<BufferHandle>,
    deferred_content_fence_releases: BTreeSet<FenceHandle>,
    software_present_frames_waiting: VecDeque<software_present::LiveProductionSoftwarePresentFrame>,
    software_present_frames_bound: BTreeMap<
        LiveProductionNativeFrameId,
        software_present::LiveProductionSoftwarePresentBinding,
    >,
    software_present_frame_owners:
        BTreeMap<LiveProductionNativeFrameId, LiveProductionNativeFrameId>,
    software_presents_unframed: VecDeque<Vec<LiveProductionSoftwarePresentSubmission>>,
    retired_software_presents: VecDeque<LiveProductionRetiredSoftwarePresent>,
    retired_software_presents_overflowed: bool,
    displayed_surfaces: BTreeMap<SurfaceId, LiveDisplayedSurface>,
    presentation_order: Vec<SurfaceId>,
    surface_outputs: BTreeMap<SurfaceId, OutputId>,
    geometry_routed_surfaces: BTreeSet<SurfaceId>,
    retained_projection_pending: bool,
    ordinary_repaints_pending: BTreeSet<OutputId>,
    content_layout_generation: u64,
    /// Output-local shell candidates and the exact grant that owns each
    /// physical retirement. Pixel equality or a replacement connection cannot
    /// settle that protocol obligation.
    retained_projection_retirements: BTreeMap<ShellContentKey, sophia_protocol::ContentGrant>,
    translations: TranslationTimeline,
    translation_origin: Instant,
    translation_deadlines: BTreeMap<OutputId, Instant>,
    chrome_surfaces: Vec<SurfaceId>,
    focused_surface: Option<SurfaceId>,
    surface_chrome_style: SurfaceChromeStyle,
    floating_outline: Option<LiveFloatingOutline>,
    indicator_publication: Option<sophia_engine::PolicyIndicatorPublication>,
    descriptor_overlay: Option<sophia_engine::DescriptorOverlayProjection>,
    descriptor_overlay_interactive: bool,
    shell_content: BTreeMap<ShellContentKey, AdmittedShellContent>,
    tab_bars: Vec<sophia_engine::TabBarProjection>,
    tab_frames: BTreeMap<OutputId, sophia_engine::CompositorDamageList>,
    pending_focus_ring_observation: Option<LiveFocusRingObservation>,
    last_focus_ring_observation: Option<LiveFocusRingObservation>,
    pending_chrome_set_observation: Option<LiveChromeSetObservation>,
    last_chrome_set_observation: Option<LiveChromeSetObservation>,
    /// Per-frame companions of chrome-set changes not yet printed. Bounded by
    /// `CHROME_FRAME_OBSERVATION_CAPACITY`.
    pending_chrome_frame_observations: Vec<LiveChromeFrameObservation>,
    /// Chrome-set changes seen so far, which paces the companions.
    chrome_set_changes: u64,
    /// Retired Presents whose Engine candidate was not applied, waiting for
    /// the native service report to carry them out. Bounded; the overflow is
    /// already counted by `present_rejections`.
    discarded_presents: Vec<crate::LiveProductionDiscardedPresent>,
    present_feedback: VecDeque<crate::LivePresentFeedbackOutcome>,
    present_feedback_overflowed: bool,
    /// Per output, the Present whose own buffer is on the screen right now.
    ///
    /// A directly scanned frame completes without idling, because the client
    /// still owns pixels the display is reading. The entry stays here until a
    /// successor flip retires on that output -- direct or composed, either is
    /// a successor -- and only then is the buffer idled back to the client.
    /// See `PresentFlipOwnership.tla`, `ReleasedOnlyBySuccessor`.
    displayed_direct_presents: BTreeMap<OutputId, TransactionId>,
    present_rejections: usize,
    native_suspend_present_rejections: usize,
    topology_escalation_present_rejections: usize,
    /// Times a queued present found an output busy and deferred. Counted so the
    /// coalesced report can say how often, since a defer is invisible on its
    /// own and the difference between a handful and a flood is the difference
    /// between ordinary contention and a present that never gets a turn.
    present_output_busy_defers: u64,
    shutdown_present_rejections: usize,
    cpu_buffer_residency: Vec<u64>,
    recent_cpu_buffer_updates: VecDeque<u64>,
    last_primary_logical_target: Option<LiveProductionCpuTarget>,
    raster_requirements: sophia_engine::SurfaceRasterRequirementTracker,
    indicator_strip_cache: std::cell::RefCell<sophia_renderer_live::IndicatorStripRasterCache>,
    text_cache: std::cell::RefCell<sophia_renderer_live::CompositorTextRasterCache>,
}

const PRESENT_FEEDBACK_CAPACITY: usize = 8_192;
const RECENT_CPU_BUFFER_UPDATE_CAPACITY: usize = 16;

pub struct LiveProductionCycleRequest<'a> {
    pub batch: &'a LiveProductionAuthorityBatch,
    pub scene: &'a mut LiveProductionCpuScene,
    pub raised_surface: Option<SurfaceId>,
    pub focused_surface: Option<SurfaceId>,
    pub cursor_presentation: LiveProductionCursorPresentation,
    pub defer_frame: bool,
    pub output_descriptors: &'a [sophia_engine::HeadlessOutput],
    pub native_scanout: Option<&'a mut LiveProductionNativeScanout>,
    pub wm_update: Option<WmTransactionUpdate>,
    pub presentation_layout: &'a [LayerSnapshot],
    /// Visible frontend-positioned surfaces, explicitly authorized by the session.
    /// All other surfaces require a policy output assignment.
    pub geometry_routed_surfaces: &'a [SurfaceId],
    pub chrome_surfaces: &'a [SurfaceId],
    pub indicator_publication: Option<sophia_engine::PolicyIndicatorPublication>,
    pub staged_cpu_buffer_handles: &'a [u64],
}

pub struct LiveAuthorityTransactionRun<'a> {
    pub groups: &'a [LiveProductionAuthorityGroup],
    pub event_count: usize,
    pub native_scanout: Option<&'a mut LiveProductionNativeScanout>,
    pub native_head_frames: Option<Vec<(OutputId, Vec<crate::LiveProductionHeadCompositionFrame>)>>,
    pub wm_update: Option<WmTransactionUpdate>,
}

include!("production_visual_runtime/configuration.rs");
include!("production_visual_runtime/cpu_cycle.rs");
include!("production_visual_runtime/gpu_cycle.rs");
include!("production_visual_runtime/presentation_layout.rs");
include!("production_visual_runtime/authority_batch.rs");
include!("production_visual_runtime/cpu_repaint.rs");
fn compositor_tick_input(
    layer_templates: &[LayerSnapshot],
    x_event_count: usize,
    authority_commits: Vec<TransactionCommit>,
    wm_update: Option<WmTransactionUpdate>,
) -> CompositorBackendTickInput {
    CompositorBackendTickInput {
        x_event_count: u32::try_from(x_event_count).unwrap_or(u32::MAX),
        authority_commits,
        authority_batches: Vec::new(),
        wm_update,
        portal_commands: Vec::new(),
        chrome_command_count: 0,
        layer_templates: layer_templates.to_vec(),
        scanout_submit_state: None,
        scanout_lifecycle_states: Vec::new(),
    }
}

fn compositor_tick_input_for_committed(
    committed: &[CommittedSurfaceState],
    surface_metadata: &BTreeMap<SurfaceId, projection::LiveSurfaceProjectionMetadata>,
    x_event_count: usize,
    authority_commits: Vec<TransactionCommit>,
    wm_update: Option<WmTransactionUpdate>,
) -> CompositorBackendTickInput {
    let layer_templates = projection::committed_layer_snapshots(committed, surface_metadata);
    compositor_tick_input(
        &layer_templates,
        x_event_count,
        authority_commits,
        wm_update,
    )
}

fn authority_transaction_count_for_groups(groups: &[LiveProductionAuthorityGroup]) -> usize {
    groups.iter().map(|group| group.transactions.len()).sum()
}

fn rebase_authority_groups_to_committed(
    groups: Vec<LiveProductionAuthorityGroup>,
    committed: &[CommittedSurfaceState],
) -> Vec<LiveProductionAuthorityGroup> {
    let mut generations = committed
        .iter()
        .map(|state| (state.surface, state.committed_generation))
        .collect::<BTreeMap<_, _>>();
    groups
        .into_iter()
        .map(|mut group| {
            for transaction in &mut group.transactions {
                let generation = generations.get(&transaction.surface).copied().unwrap_or(0);
                transaction.previous_committed_generation = generation;
                generations.insert(transaction.surface, generation.saturating_add(1));
            }
            for surface in &group.removed_surfaces {
                generations.remove(surface);
            }
            group
        })
        .collect()
}

fn write_cpu_buffer_residency<'a>(
    handles: &mut Vec<u64>,
    committed: &[CommittedSurfaceState],
    batch: &LiveProductionAuthorityBatch,
    pending_groups: impl Iterator<Item = &'a LiveProductionAuthorityGroup>,
    scheduled_present_handles: impl Iterator<Item = u64>,
    staged: &[u64],
    recent_updates: &VecDeque<u64>,
) {
    handles.clear();
    handles.extend(
        committed
            .iter()
            .flat_map(|surface| surface.content.variants())
            .filter_map(|variant| match variant.source {
                BufferSource::CpuBuffer { handle } => Some(handle),
                _ => None,
            }),
    );
    handles.extend(
        batch
            .groups
            .iter()
            .flat_map(|group| group.transactions.iter())
            .flat_map(|transaction| transaction.content.variants())
            .filter_map(|variant| match variant.source {
                BufferSource::CpuBuffer { handle } => Some(handle),
                _ => None,
            }),
    );
    handles.extend(
        pending_groups
            .flat_map(|group| group.transactions.iter())
            .flat_map(|transaction| transaction.content.variants())
            .filter_map(|variant| match variant.source {
                BufferSource::CpuBuffer { handle } => Some(handle),
                _ => None,
            }),
    );
    handles.extend(scheduled_present_handles);
    handles.extend_from_slice(staged);
    handles.extend(recent_updates);
    handles.sort_unstable();
    handles.dedup();
}

fn authority_batch_cpu_buffer_updates(
    batch: &LiveProductionAuthorityBatch,
) -> Vec<crate::LiveCpuBufferUpdate> {
    batch
        .groups
        .iter()
        .flat_map(|group| {
            group
                .cpu_buffer_updates
                .iter()
                .map(|update| update.update.clone())
        })
        .collect()
}

fn authority_batch_cpu_progress(batch: &LiveProductionAuthorityBatch) -> LiveProductionCpuProgress {
    let mut progress = LiveProductionCpuProgress::default();
    for group in &batch.groups {
        for update in &group.cpu_buffer_updates {
            progress.accepted_updates = progress.accepted_updates.saturating_add(1);
            progress.latest_update = Some(update.identity);
        }
        progress
            .removed_surfaces
            .extend(group.removed_surfaces.iter().copied());
    }
    progress
}

fn authority_group_present_owners(
    group: &LiveProductionAuthorityGroup,
) -> Result<Vec<SurfaceTransactionKey>, &'static str> {
    let mut owners = group
        .software_present_submissions
        .iter()
        .map(|submission| submission.candidate)
        .collect::<Vec<_>>();
    for submission in &group.present_submissions {
        let mut candidates = group.transactions.iter().filter(|transaction| {
            transaction.transaction == submission.transaction
                && transaction.surface == submission.surface
                && transaction.target_buffer()
                    == BufferSource::DmaBuf {
                        handle: submission.buffer.raw(),
                    }
        });
        let owner = candidates
            .next()
            .ok_or("DMA-BUF Present has no exact content owner")?
            .key();
        if candidates.next().is_some() {
            return Err("DMA-BUF Present has multiple content owners");
        }
        owners.push(owner);
    }
    Ok(owners)
}

fn record_recent_cpu_buffer_updates(
    recent: &mut VecDeque<u64>,
    updates: &[crate::LiveCpuBufferUpdate],
) {
    for handle in updates.iter().map(crate::LiveCpuBufferUpdate::handle) {
        if let Some(index) = recent.iter().position(|candidate| *candidate == handle) {
            recent.remove(index);
        }
        recent.push_back(handle);
    }
    while recent.len() > RECENT_CPU_BUFFER_UPDATE_CAPACITY {
        recent.pop_front();
    }
}

fn retain_relevant_cpu_buffer_updates(
    scene: &LiveProductionCpuScene,
    updates: &mut Vec<crate::LiveCpuBufferUpdate>,
    rooted_handles: &[u64],
) {
    updates.retain(|update| {
        matches!(update, crate::LiveCpuBufferUpdate::Replace(_))
            || scene.contains_buffer(update.handle())
            || rooted_handles.binary_search(&update.handle()).is_ok()
    });
}

fn authority_batch_removed_surfaces(batch: &LiveProductionAuthorityBatch) -> Vec<SurfaceId> {
    batch
        .groups
        .iter()
        .flat_map(|group| group.removed_surfaces.iter().copied())
        .collect()
}

pub fn live_production_transactions_require_gpu_scanout(
    transactions: &[SurfaceTransaction],
) -> bool {
    transactions
        .iter()
        .any(|transaction| matches!(transaction.target_buffer(), BufferSource::DmaBuf { .. }))
}

pub fn live_production_projection_requires_gpu_scanout(
    transactions: &[SurfaceTransaction],
    presentation_order: &[SurfaceId],
) -> bool {
    transactions.iter().any(|transaction| {
        presentation_order.contains(&transaction.surface)
            && matches!(transaction.target_buffer(), BufferSource::DmaBuf { .. })
    })
}

fn live_production_committed_projection_requires_gpu_scanout(
    committed: &[CommittedSurfaceState],
    presentation_order: &[SurfaceId],
) -> bool {
    committed.iter().any(|state| {
        presentation_order.contains(&state.surface)
            && matches!(state.buffer(), BufferSource::DmaBuf { .. })
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveProductionMixedLayerSource {
    CurrentDmaBuf,
    Cpu(SurfaceId),
    RetainedDmaBuf(SurfaceId),
}

pub fn live_production_mixed_layer_order(
    presentation_order: &[SurfaceId],
    current: SurfaceId,
    cpu_surfaces: &[SurfaceId],
    retained_dma_buf_surfaces: &[SurfaceId],
) -> Vec<LiveProductionMixedLayerSource> {
    presentation_order
        .iter()
        .filter_map(|surface| {
            if *surface == current {
                Some(LiveProductionMixedLayerSource::CurrentDmaBuf)
            } else if cpu_surfaces.contains(surface) {
                Some(LiveProductionMixedLayerSource::Cpu(*surface))
            } else if retained_dma_buf_surfaces.contains(surface) {
                Some(LiveProductionMixedLayerSource::RetainedDmaBuf(*surface))
            } else {
                None
            }
        })
        .collect()
}

pub const fn reduce_live_production_frame_defer(
    requested_defer: bool,
    presentation_order_changed: bool,
    preserved_gpu_projection: bool,
) -> bool {
    preserved_gpu_projection || (requested_defer && !presentation_order_changed)
}

pub const fn live_production_retained_projection_admitted(
    visual_projection_changed: bool,
    current_cpu_updates: bool,
    committed_projection_requires_gpu: bool,
) -> bool {
    visual_projection_changed && (!current_cpu_updates || committed_projection_requires_gpu)
}

pub const fn live_production_should_preserve_gpu_output(
    native_enabled: bool,
    gpu_present_submitted: bool,
    retained_projection_queued: bool,
    _presentation_order_changed: bool,
    committed_projection_requires_gpu: bool,
) -> bool {
    // GPU visibility is already evaluated against the new presentation order.
    // Retained queueing may be suppressed because the exact frame is already
    // owned; zero newly queued frames cannot make its DMA-BUFs CPU-readable.
    native_enabled
        && (gpu_present_submitted
            || retained_projection_queued
            || committed_projection_requires_gpu)
}
