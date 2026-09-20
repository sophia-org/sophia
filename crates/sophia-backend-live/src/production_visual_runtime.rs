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

impl LiveProductionVisualRuntime {
    pub const fn focused_surface(&self) -> Option<SurfaceId> {
        self.focused_surface
    }

    pub fn new(
        outputs: &[sophia_engine::HeadlessOutput],
        native_scanout: Option<&mut LiveProductionNativeScanout>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let production = sophia_engine::ProductionSessionCoordinator::new(
            sophia_engine::HeadlessEngine::default(),
        );
        let output_runtimes = LiveProductionOutputRuntimeSet::new(outputs, &[], native_scanout)?;
        let input_projections = (0..output_runtimes.output_count())
            .filter_map(|index| output_runtimes.output_id(index))
            .map(|output| LivePresentedInputProjection {
                output,
                epoch: 0,
                layers: Vec::new(),
                chrome_targets: Vec::new(),
                chrome_occlusion: None,
                descriptor_targets: Vec::new(),
                descriptor_occlusion: None,
                descriptor_projection: None,
                tab_occlusions: Vec::new(),
                content: Vec::new(),
            })
            .collect();
        Ok(Self {
            native_suspended: false,
            production,
            outputs: output_runtimes,
            surface_metadata: BTreeMap::new(),
            input_projections,
            presentation_feedback: Default::default(),
            present_scheduler: LiveProductionPresentScheduler::default(),
            surface_content_stream: SurfaceContentStream::default(),
            released_surface_content: VecDeque::new(),
            superseded_surface_content: VecDeque::new(),
            deferred_content_dma_buf_releases: BTreeSet::new(),
            deferred_content_fence_releases: BTreeSet::new(),
            software_present_frames_waiting: VecDeque::new(),
            software_present_frames_bound: BTreeMap::new(),
            software_present_frame_owners: BTreeMap::new(),
            software_presents_unframed: VecDeque::new(),
            retired_software_presents: VecDeque::with_capacity(PRESENT_FEEDBACK_CAPACITY),
            retired_software_presents_overflowed: false,
            displayed_surfaces: BTreeMap::new(),
            presentation_order: Vec::new(),
            surface_outputs: BTreeMap::new(),
            geometry_routed_surfaces: BTreeSet::new(),
            retained_projection_pending: false,
            ordinary_repaints_pending: BTreeSet::new(),
            content_layout_generation: 1,
            retained_projection_retirements: BTreeMap::new(),
            translations: TranslationTimeline::default(),
            translation_origin: Instant::now(),
            translation_deadlines: BTreeMap::new(),
            chrome_surfaces: Vec::new(),
            focused_surface: None,
            surface_chrome_style: SurfaceChromeStyle::default(),
            floating_outline: None,
            indicator_publication: None,
            descriptor_overlay: None,
            descriptor_overlay_interactive: false,
            shell_content: BTreeMap::new(),
            tab_bars: Vec::new(),
            tab_frames: BTreeMap::new(),
            pending_focus_ring_observation: None,
            last_focus_ring_observation: None,
            pending_chrome_set_observation: None,
            last_chrome_set_observation: None,
            pending_chrome_frame_observations: Vec::new(),
            chrome_set_changes: 0,
            discarded_presents: Vec::new(),
            present_feedback: VecDeque::with_capacity(PRESENT_FEEDBACK_CAPACITY),
            present_feedback_overflowed: false,
            displayed_direct_presents: BTreeMap::new(),
            present_rejections: 0,
            native_suspend_present_rejections: 0,
            topology_escalation_present_rejections: 0,
            present_output_busy_defers: 0,
            shutdown_present_rejections: 0,
            cpu_buffer_residency: Vec::with_capacity(16),
            recent_cpu_buffer_updates: VecDeque::with_capacity(RECENT_CPU_BUFFER_UPDATE_CAPACITY),
            last_primary_logical_target: None,
            raster_requirements: Default::default(),
            indicator_strip_cache: Default::default(),
            text_cache: Default::default(),
        })
    }

    /// Derives the union of native-density classes required by every visible
    /// physical head. This is an Engine reducer; the backend merely supplies
    /// current targets and retains no X11 identity.
    pub fn reconcile_surface_raster_requirements(
        &mut self,
        native_scanout: &LiveProductionNativeScanout,
    ) -> Result<Vec<SurfaceRasterRequirements>, Box<dyn std::error::Error>> {
        let committed = self.production.committed_surfaces();
        let scene_generation = committed
            .iter()
            .map(|state| state.committed_generation)
            .max()
            .unwrap_or(1)
            .max(1);
        let mut snapshots = Vec::new();
        let mut targets = Vec::new();
        for (output, logical_viewport) in self.outputs.logical_viewports() {
            let display_list = self.display_list_for_output(
                output,
                logical_viewport,
                committed,
                &self.presentation_order,
            )?;
            snapshots.push(sophia_engine::output_scene_snapshot_from_committed_in_view(
                output,
                scene_generation,
                logical_viewport,
                committed,
                display_list,
                None,
            )?);
            targets.extend(native_scanout.head_render_targets(output));
        }
        self.raster_requirements
            .reconcile(&snapshots, &targets)
            .map_err(Into::into)
    }

    pub fn accept_surface_raster_response(
        &mut self,
        identity: SurfaceRasterResponseIdentity,
    ) -> bool {
        self.raster_requirements.accept_response(identity)
    }

    pub fn stable_present(
        &self,
        native_scanout: &LiveProductionNativeScanout,
        transaction: TransactionId,
        outputs: &[OutputId],
    ) -> bool {
        !outputs.is_empty()
            && outputs
                .iter()
                .all(|output| native_scanout.stable_present(*output, transaction))
    }

    pub fn with_m4_proof_controls(
        mut self,
        first_acquire_delay: Option<Duration>,
        reject_first_present: bool,
        diagnose_first_mixed_export: bool,
    ) -> Self {
        self.present_scheduler = self.present_scheduler.with_controls(
            first_acquire_delay,
            reject_first_present,
            diagnose_first_mixed_export,
        );
        self
    }

    pub fn with_surface_chrome_style(mut self, style: SurfaceChromeStyle) -> Self {
        self.surface_chrome_style = style;
        self
    }

    pub fn set_surface_chrome_style(&mut self, style: SurfaceChromeStyle) -> bool {
        if self.surface_chrome_style == style {
            return false;
        }
        self.surface_chrome_style = style;
        self.last_focus_ring_observation = None;
        self.last_chrome_set_observation = None;
        true
    }

    pub fn set_indicator_publication(
        &mut self,
        publication: Option<sophia_engine::PolicyIndicatorPublication>,
    ) -> bool {
        if self.indicator_publication == publication {
            return false;
        }
        self.indicator_publication = publication;
        true
    }

    /// The surface whose chrome should read as focused.
    ///
    /// Input focus follows menus, tooltips and other popups, and those carry
    /// no chrome of their own. Reporting one as the focused surface matches no
    /// framed window, so every window's border repaints in the unfocused
    /// colour for as long as the popup lives, then snaps back -- a visible
    /// flash on every menu. A popup belongs to the window that opened it, so
    /// focus landing on an unframed surface holds the framed surface it came
    /// from. Focus genuinely going nowhere still clears it, so clicking away
    /// from every window unfocuses them all.
    ///
    /// The held value was resolved the same way when it was stored, so this
    /// cannot chain through a run of popups back to something unframed. It is
    /// re-checked against the incoming chrome set regardless, because the
    /// window a popup belonged to can lose its chrome while the popup is up.
    fn chrome_focus(
        &self,
        focused_surface: Option<SurfaceId>,
        chrome_surfaces: &[SurfaceId],
    ) -> Option<SurfaceId> {
        match focused_surface {
            Some(surface) if chrome_surfaces.contains(&surface) => Some(surface),
            Some(_) => self
                .focused_surface
                .filter(|held| chrome_surfaces.contains(held)),
            None => None,
        }
    }

    /// Resolves a repaint's chrome focus and prepares its display list.
    ///
    /// `raised_surface` orders the stack; `focused_surface` is the focus. They
    /// are independent: a raise never becomes the focus, framed or not.
    fn prepare_repaint(
        &mut self,
        committed: &[CommittedSurfaceState],
        raised_surface: Option<SurfaceId>,
        focused_surface: Option<SurfaceId>,
    ) -> Result<CompositorDisplayList, CompositorDisplayListError> {
        let focus = self.chrome_focus(focused_surface, &self.chrome_surfaces);
        self.focused_surface = focus;
        let presentation_order =
            raised_presentation_order(&self.presentation_order, raised_surface);
        self.display_list(committed, &presentation_order)
    }

    pub fn run_cpu_production_cycle(
        &mut self,
        request: LiveProductionCycleRequest<'_>,
    ) -> Result<CpuCycleOutcome, Box<dyn std::error::Error>> {
        let LiveProductionCycleRequest {
            batch,
            scene,
            raised_surface,
            focused_surface,
            cursor_presentation,
            defer_frame,
            output_descriptors,
            mut native_scanout,
            wm_update,
            presentation_layout,
            geometry_routed_surfaces,
            chrome_surfaces,
            indicator_publication,
            staged_cpu_buffer_handles,
        } = request;
        let authority_envelope = batch;
        authority_envelope.validate()?;
        self.presentation_feedback
            .observe_authority_resource_registrations(authority_envelope)?;
        let batch = self.ready_surface_content_batch(authority_envelope)?;
        let _ = self.reject_superseded_surface_content()?;
        let mut cpu_progress = authority_batch_cpu_progress(&batch);
        let mut updates = authority_batch_cpu_buffer_updates(&batch);
        record_recent_cpu_buffer_updates(&mut self.recent_cpu_buffer_updates, &updates);
        write_cpu_buffer_residency(
            &mut self.cpu_buffer_residency,
            self.production.committed_surfaces(),
            &batch,
            self.surface_content_stream
                .deferred_items()
                .chain(self.released_surface_content.iter()),
            self.present_scheduler.retained_cpu_buffer_handles(),
            staged_cpu_buffer_handles,
            &self.recent_cpu_buffer_updates,
        );
        retain_relevant_cpu_buffer_updates(scene, &mut updates, &self.cpu_buffer_residency);
        let native_enabled = native_scanout.is_some();
        let focused_surface = self.chrome_focus(focused_surface, chrome_surfaces);
        let focus_changed = self.focused_surface != focused_surface;
        self.focused_surface = focused_surface;
        let presentation_layout_changed =
            self.apply_presentation_layout(presentation_layout, geometry_routed_surfaces);
        if let Some(native) = native_scanout.as_deref_mut() {
            native.set_translation_motion_active(self.translations.active(self.translation_time()));
        }
        let chrome_surfaces_changed = self.set_chrome_surfaces(chrome_surfaces);
        let indicator_publication_changed = self.set_indicator_publication(indicator_publication);
        let visual_projection_changed = presentation_layout_changed
            || chrome_surfaces_changed
            || focus_changed
            || indicator_publication_changed;
        let committed_projection_requires_gpu =
            live_production_committed_projection_requires_gpu_scanout(
                self.production.committed_surfaces(),
                &self.presentation_order,
            );
        // A retained CPU layer is a snapshot from before this authority batch is
        // committed. Let a CPU-only scene compose the current updates instead of
        // placing new chrome around stale client pixels. GPU-owned projections
        // still need the retained mixed path to preserve their image ownership.
        let retained_projection_queued = if live_production_retained_projection_admitted(
            visual_projection_changed,
            !updates.is_empty(),
            committed_projection_requires_gpu,
        ) {
            match native_scanout.as_deref_mut() {
                Some(native_scanout) => self.queue_retained_projection(scene, native_scanout)?,
                None => false,
            }
        } else {
            false
        };
        if visual_projection_changed {
            tracing::debug!(
                "sophia_live_retained_projection schema=2 status={} focus_changed={} layout_changed={} chrome_changed={}",
                if retained_projection_queued {
                    "queued"
                } else {
                    "unavailable"
                },
                focus_changed,
                presentation_layout_changed,
                chrome_surfaces_changed,
            );
        }
        let removed_surfaces = authority_batch_removed_surfaces(&batch);
        self.release_removed_presentations(&removed_surfaces, native_scanout.as_deref_mut())?;
        let rebased_groups = batch.groups;
        self.enqueue_software_presents(&rebased_groups)?;
        let software_present_frame_required = !self.software_presents_unframed.is_empty();
        for group in &rebased_groups {
            self.observe_surface_metadata(&group.transactions, &group.removed_surfaces);
        }
        self.displayed_surfaces
            .retain(|surface, _| !removed_surfaces.contains(surface));
        let preserve_gpu_scanout = live_production_should_preserve_gpu_output(
            native_scanout.is_some(),
            self.present_scheduler.has_in_flight(),
            retained_projection_queued,
            presentation_layout_changed,
            committed_projection_requires_gpu,
        );
        let defer_frame = if software_present_frame_required {
            false
        } else {
            reduce_live_production_frame_defer(
                defer_frame,
                visual_projection_changed,
                preserve_gpu_scanout,
            )
        };
        let native_scanout = if preserve_gpu_scanout || software_present_frame_required {
            None
        } else {
            native_scanout
        };
        let intakes = rebased_groups
            .iter()
            .map(|group| {
                AuthorityTransactionIntake::new(group.transaction, group.transactions.clone())
                    .with_surface_removals(group.removed_surfaces.clone())
            })
            .collect::<Vec<_>>();
        self.observe_content_ordered_resource_releases(authority_envelope);
        let head_plan_composition = output_composition::OutputCompositionSnapshot::capture(self);
        let ordinary_repaints_pending = &mut self.ordinary_repaints_pending;
        let (production, outputs) = (&mut self.production, &mut self.outputs);
        let output_count = outputs.output_count();
        let primary_output = outputs.primary_output();
        let event_count = authority_transaction_count_for_groups(&rebased_groups);
        let surface_metadata = self.surface_metadata.clone();
        let indicator_strip_cache = &self.indicator_strip_cache;
        let text_cache = &self.text_cache;
        let mut native_scanout = native_scanout;
        let create_native_frames = native_scanout.is_some();
        // Native head frames are never composed from the committed set while a
        // Present owns the scanout: the preservation rule above withheld the
        // scanout. The chrome invariant below leans on this, so it is stated.
        debug_assert!(!create_native_frames || !self.present_scheduler.has_in_flight());
        let primary_logical_target = std::cell::Cell::new(None);
        let primary_logical_target_ref = &primary_logical_target;
        let mut adapter = LiveProductionCpuCycleAdapter::new(
            scene,
            &self.presentation_order,
            &self.chrome_surfaces,
            updates,
            raised_surface,
            focused_surface,
            self.surface_chrome_style,
            cursor_presentation.composition_position(),
            defer_frame,
            create_native_frames,
            &self.cpu_buffer_residency,
            output_descriptors,
            move |cycle: u64,
                  committed: &[CommittedSurfaceState],
                  authority_commits: &[TransactionCommit],
                  native_frames: Option<Vec<LiveProductionComposedFrame>>,
                  cpu_layers: Vec<LiveCpuPresentationLayer>| {
                // A deferred cycle may service retained native work without
                // producing a new CPU frame; only an actual frame set initializes outputs.
                let initialize_native = native_frames.is_some();
                let mut output_adapter = crate::LiveProductionOutputRuntimeAdapter::new(
                    output_count,
                    |index,
                     snapshot: &[CommittedSurfaceState]|
                     -> Result<_, Box<dyn std::error::Error>> {
                        let output_id = outputs
                            .output_id(index)
                            .ok_or("production output index was not registered")?;
                        let logical_viewport = outputs
                            .logical_viewport(output_id)
                            .ok_or("production output logical viewport was not registered")?;
                        let needs_initialization = initialize_native
                            && native_scanout.is_some()
                            && !outputs.native_initialized(output_id);
                        let mut initialized_here = false;
                        let result = outputs.run_output(index, snapshot, |runtime| {
                            let input = compositor_tick_input_for_committed(
                                snapshot,
                                &surface_metadata,
                                event_count,
                                authority_commits.to_vec(),
                                wm_update.clone(),
                            );
                            Ok(match native_scanout.as_deref_mut() {
                                Some(native_scanout) => {
                                    let display_list = head_plan_composition.display_list(output_id, snapshot)?;
                                    let scene = sophia_engine::output_scene_snapshot_from_committed_in_view(
                                        output_id,
                                        cycle.max(1),
                                        logical_viewport,
                                        snapshot,
                                        display_list,
                                        None,
                                    )?;
                                    let targets = native_scanout.head_render_targets(output_id);
                                    let plans = sophia_engine::build_output_head_plans(
                                        &scene,
                                        &targets,
                                    )?;
                                    if plans.len() != targets.len() {
                                        return Err(
                                            "native head planner returned partial target coverage"
                                                .into(),
                                        );
                                    }
                                    for plan in &plans {
                                        trace_live_head_composition_plan(plan);
                                    }
                                    if initialize_native {
                                        let logical_target = plans
                                            .first()
                                            .map(|plan| plan.logical_content_checksum);
                                        if plans.iter().any(|plan| {
                                            Some(plan.logical_content_checksum) != logical_target
                                        }) {
                                            return Err(
                                                "native heads disagree on logical content checksum"
                                                    .into(),
                                            );
                                        }
                                        let prepared = plans
                                            .iter()
                                            .map(|plan| {
                                                Ok(crate::LiveProductionHeadCompositionFrame {
                                                    head: plan.head,
                                                    scene_generation: plan.scene_generation,
                                                    target_generation: plan.target_generation,
                                                    mapping: plan.mapping,
                                                    logical_content_checksum: plan
                                                        .logical_content_checksum,
                                                    frame: sophia_renderer_live::lower_cpu_head_composition_plan_with_caches(
                                                        plan,
                                                        &cpu_layers,
                                                        &mut indicator_strip_cache.borrow_mut(),
                                                        &mut text_cache.borrow_mut(),
                                                    )?,
                                                })
                                            })
                                            .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>(
                                            )?;
                                        let queued_target = if needs_initialization {
                                            native_scanout.initialize_head_composition(
                                                output_id,
                                                runtime,
                                                prepared,
                                            )?;
                                            initialized_here = true;
                                            None
                                        } else {
                                            let frame = ordinary_repaint::admit(ordinary_repaints_pending, native_scanout, output_id, prepared)?;
                                            frame.and_then(|frame| logical_target.map(|checksum| {
                                                LiveProductionCpuTarget::new(frame, checksum)
                                            }))
                                        };
                                        if Some(output_id) == primary_output {
                                            primary_logical_target_ref.set(queued_target);
                                        }
                                    }
                                    if runtime.rendered_primary_plane_scanout_in_flight() {
                                        runtime.run_tick(input)?
                                    } else {
                                        native_scanout.run_tick(output_id, runtime, input)?
                                    }
                                }
                                None => runtime.run_tick(input)?,
                            })
                        });
                        if result.is_ok() && initialized_here {
                            outputs.mark_native_initialized(output_id)?;
                        }
                        result
                    },
                );
                (0..output_count)
                    .map(|index| output_adapter.run_output(index, committed))
                    .collect::<Result<Vec<_>, _>>()?
                    .into_iter()
                    .next()
                    .ok_or_else(|| "persistent backend runtime has no outputs".into())
            },
        );
        let report = production
            .run_cycle(&intakes, &mut adapter)
            .map_err(|error| {
                format!(
                    "production CPU cycle failed in phase {:?}: {}",
                    error.phase, error.source
                )
            })?;
        drop(adapter);
        cpu_progress.bind_primary_logical_target(primary_logical_target.get());
        if software_present_frame_required {
            if !report.submission.composed {
                return Err("software Present did not produce an immutable composed frame".into());
            }
            if native_enabled {
                self.frame_unframed_software_presents(scene, output_descriptors)?;
            } else if self.native_suspended {
                self.reject_software_presents();
            } else {
                self.settle_unframed_software_presents_without_native()?;
            }
        }
        if report.submission.composed {
            // Chrome follows what the heads show. While a Present owns the
            // scanout its candidate is on screen, not the set this turn's
            // intakes committed; observing the latter here and the former on
            // the Present turn framed one stale surface in two places on
            // alternate scanouts.
            let displayed = self.displayed_surface_view().to_vec();
            self.record_focus_ring_observation(
                &displayed,
                LiveChromeObservationSource::Production,
                false,
            )?;
        }
        // Native input advances only when retire_native_scanout_output
        // observes the corresponding accepted page flip.
        if !native_enabled && !self.native_suspended {
            self.publish_committed_input_layers();
        }
        Ok((report.submission, report.committed_surfaces, cpu_progress))
    }

    pub fn run_gpu_production_cycle(
        &mut self,
        request: LiveProductionCycleRequest<'_>,
    ) -> Result<
        (
            LiveProductionCpuSubmission,
            Vec<CommittedSurfaceState>,
            LiveProductionCpuProgress,
        ),
        Box<dyn std::error::Error>,
    > {
        let LiveProductionCycleRequest {
            batch,
            scene,
            raised_surface,
            focused_surface,
            cursor_presentation,
            defer_frame,
            output_descriptors,
            native_scanout,
            wm_update,
            presentation_layout,
            geometry_routed_surfaces,
            chrome_surfaces,
            indicator_publication,
            staged_cpu_buffer_handles,
        } = request;
        let native_enabled = native_scanout.is_some();
        let batch = self.ready_surface_content_batch(batch)?;
        self.last_primary_logical_target = None;
        let mut cpu_progress = authority_batch_cpu_progress(&batch);
        let mut updates = authority_batch_cpu_buffer_updates(&batch);
        record_recent_cpu_buffer_updates(&mut self.recent_cpu_buffer_updates, &updates);
        write_cpu_buffer_residency(
            &mut self.cpu_buffer_residency,
            self.production.committed_surfaces(),
            &batch,
            self.surface_content_stream
                .deferred_items()
                .chain(self.released_surface_content.iter()),
            self.present_scheduler.retained_cpu_buffer_handles(),
            staged_cpu_buffer_handles,
            &self.recent_cpu_buffer_updates,
        );
        retain_relevant_cpu_buffer_updates(scene, &mut updates, &self.cpu_buffer_residency);
        self.focused_surface = self.chrome_focus(focused_surface, chrome_surfaces);
        let _ = self.apply_presentation_layout(presentation_layout, geometry_routed_surfaces);
        self.set_chrome_surfaces(chrome_surfaces);
        self.set_indicator_publication(indicator_publication);
        let committed_surfaces = self.committed_surfaces().to_vec();
        scene.apply_production_updates(updates)?;
        scene.reconcile_buffer_residency(&self.cpu_buffer_residency);
        let missing_buffers = scene.missing_committed_buffer_count(&committed_surfaces);
        if missing_buffers != 0 {
            return Err(format!(
                "production GPU scene is missing {missing_buffers} committed CPU buffer(s)"
            )
            .into());
        }
        let compose_started = Instant::now();
        let mut composition = if defer_frame {
            scene
                .last_report()
                .cloned()
                .ok_or("software redraw coalescing has no prior composed frame")?
        } else {
            let presentation_order =
                raised_presentation_order(&self.presentation_order, raised_surface);
            let display_list = self.display_list(&committed_surfaces, &presentation_order)?;
            let output = output_descriptors
                .first()
                .copied()
                .ok_or("software composition has no output descriptor")?;
            scene
                .compose_display_list(
                    output,
                    &committed_surfaces,
                    &display_list,
                    cursor_presentation.composition_position(),
                )?
                .clone()
        };
        // A Present-bearing authority group owns the next native visual
        // candidate. Do not queue a retained CPU frame ahead of it: that can
        // expose new layout/chrome around old or absent client pixels.
        let native_frames = if defer_frame || batch.has_dma_buf_present_submissions() {
            None
        } else {
            native_scanout
                .as_ref()
                .map(|_| scene.frames_for_outputs(output_descriptors))
                .transpose()?
        };
        let cpu_layers =
            scene.presentation_variant_layers(&committed_surfaces, &self.presentation_order);
        let tick = self.run_batch(
            &batch,
            presentation_layout,
            if defer_frame { None } else { native_scanout },
            native_frames,
            scene,
            cpu_layers,
            wm_update,
        )?;
        let software_present_frame_required = !self.software_presents_unframed.is_empty();
        cpu_progress.bind_primary_logical_target(self.last_primary_logical_target);
        if software_present_frame_required {
            let committed_surfaces = self.committed_surfaces().to_vec();
            let presentation_order =
                raised_presentation_order(&self.presentation_order, raised_surface);
            let display_list = self.display_list(&committed_surfaces, &presentation_order)?;
            let output = output_descriptors
                .first()
                .copied()
                .ok_or("software Present has no output descriptor")?;
            composition = scene
                .compose_display_list(
                    output,
                    &committed_surfaces,
                    &display_list,
                    cursor_presentation.composition_position(),
                )?
                .clone();
            if native_enabled {
                self.frame_unframed_software_presents(scene, output_descriptors)?;
            } else if self.native_suspended {
                self.reject_software_presents();
            } else {
                self.settle_unframed_software_presents_without_native()?;
            }
        }
        Ok((
            LiveProductionCpuSubmission {
                tick,
                composition,
                composed: !defer_frame || software_present_frame_required,
                compose_elapsed: if defer_frame && !software_present_frame_required {
                    Duration::ZERO
                } else {
                    compose_started.elapsed()
                },
                primary_logical_target: cpu_progress.primary_logical_target,
            },
            committed_surfaces,
            cpu_progress,
        ))
    }

    fn apply_presentation_layout(
        &mut self,
        layout: &[LayerSnapshot],
        geometry_routed: &[SurfaceId],
    ) -> bool {
        let now = Instant::now();
        let time = self.translation_time();
        let translation_changed = self.translations.replace_targets(layout, time);
        if translation_changed {
            tracing::debug!(
                event = "translation_targets",
                active = self.translations.active(time),
                members = layout
                    .iter()
                    .filter(|layer| layer.translation.is_some())
                    .count(),
                "updated Engine presentation translation targets"
            );
        }
        for output in self.outputs.logical_viewports().map(|(id, _)| id) {
            if self.translations.active_on(output, time) {
                self.translation_deadlines.entry(output).or_insert(now);
            }
        }
        let order_changed = self.presentation_order.len() != layout.len()
            || self
                .presentation_order
                .iter()
                .zip(layout)
                .any(|(surface, layer)| *surface != layer.surface);
        self.presentation_order.clear();
        self.presentation_order
            .extend(layout.iter().map(|layer| layer.surface));
        if order_changed {
            // Input eligibility must not wait for the next accepted page flip.
            // On the native path nothing republishes the projection until a
            // frame retires, so a window unmapped now would keep answering the
            // pointer for the whole interval until then -- unbounded if the
            // flip stalls. Pruning here only ever removes what has left the
            // layout; pixels still on screen keep routing until they do.
            self.prune_input_projections_to_presentation_order();
        }
        // Which head composites each surface. A scrolling layout puts columns
        // past the edge of their own display on purpose, and with a second
        // display beside it, "past the edge" and "inside the neighbour" are
        // the same region -- so without this, geometry alone drew one
        // display's window on another.
        let routed = layout
            .iter()
            .filter(|layer| layer.output.is_none() && geometry_routed.contains(&layer.surface))
            .map(|layer| layer.surface)
            .collect::<BTreeSet<_>>();
        let routing_changed = self.geometry_routed_surfaces != routed
            || layout
                .iter()
                .any(|layer| self.surface_outputs.get(&layer.surface).copied() != layer.output);
        self.geometry_routed_surfaces = routed;
        self.surface_outputs.clear();
        for layer in layout {
            if let Some(output) = layer.output {
                self.surface_outputs.insert(layer.surface, output);
            }
        }
        for layer in layout {
            self.present_scheduler
                .reproject_surface(layer.surface, layer.geometry);
            if let Some(displayed) = self.displayed_surfaces.get_mut(&layer.surface) {
                displayed.layer.reproject(layer.geometry);
            }
        }
        // Restarted policy connections seed stationary translation targets, so
        // no animation deadline will repaint the pixels their old positions
        // occupied. Request the ordinary retained repaint even without a new
        // client frame; its existing admission barrier still owns retirement.
        order_changed || routing_changed || translation_changed
    }

    fn set_chrome_surfaces(&mut self, surfaces: &[SurfaceId]) -> bool {
        if self.chrome_surfaces == surfaces {
            return false;
        }
        self.chrome_surfaces.clear();
        self.chrome_surfaces.extend_from_slice(surfaces);
        true
    }

    pub fn run_batch(
        &mut self,
        batch: &LiveProductionAuthorityBatch,
        presentation_layout: &[LayerSnapshot],
        mut native_scanout: Option<&mut LiveProductionNativeScanout>,
        native_frames: Option<Vec<LiveProductionComposedFrame>>,
        scene: &LiveProductionCpuScene,
        cpu_layers: Vec<LiveCpuPresentationLayer>,
        wm_update: Option<WmTransactionUpdate>,
    ) -> Result<crate::LiveBackendRuntimeTickReport, Box<dyn std::error::Error>> {
        batch.validate()?;
        self.presentation_feedback
            .observe_authority_resource_registrations(batch)?;
        let _ = self.reject_superseded_surface_content()?;
        let removed_surfaces = authority_batch_removed_surfaces(batch);
        self.release_removed_presentations(&removed_surfaces, native_scanout.as_deref_mut())?;
        self.displayed_surfaces
            .retain(|surface, _| !removed_surfaces.contains(surface));
        let mut authority_groups = Vec::new();
        let mut has_present_submissions = false;
        for group in &batch.groups {
            if group.present_submissions.is_empty() {
                authority_groups.push(group.clone());
            } else {
                has_present_submissions = true;
                let superseded = self.present_scheduler.enqueue_group(
                    group,
                    presentation_layout,
                    self.presentation_feedback.resources_mut(),
                    Instant::now(),
                )?;
                for transaction in superseded {
                    self.reject_gpu_presentation(transaction);
                }
            }
        }
        // Software and DMA-BUF Presents can arrive in separate authority
        // groups in one owner batch. Queue the software feedback before the
        // GPU group drives the shared native frame so both retire on its
        // page-flip clock.
        self.enqueue_software_presents(&authority_groups)?;
        self.observe_content_ordered_resource_releases(batch);
        if has_present_submissions && !authority_groups.is_empty() {
            let prepared = self.prepare_authority_groups(&authority_groups)?;
            let _ = self.run_prepared_authority_transactions(
                prepared,
                authority_transaction_count_for_groups(&authority_groups),
                None,
                None,
                wm_update.clone(),
            )?;
        }
        if has_present_submissions {
            for group in &batch.groups {
                self.observe_surface_metadata(&group.transactions, &group.removed_surfaces);
            }
            if !self.present_scheduler.has_eligible() {
                return self.run_observation_tick();
            }
            return self.drive_gpu_presentation(scene, native_scanout.as_deref_mut());
        }
        if authority_groups.is_empty() {
            return self.run_observation_tick();
        }
        let prepared = self.prepare_authority_groups(&authority_groups)?;
        let scene_generation = self
            .production
            .committed_surfaces()
            .iter()
            .map(|state| state.committed_generation)
            .max()
            .unwrap_or(1)
            .max(1);
        let native_head_frames = if native_frames.is_some() {
            native_scanout
                .as_deref()
                .map(|native| {
                    self.cpu_output_head_composition_frames_from_layers(
                        native,
                        &cpu_layers,
                        scene_generation,
                    )
                })
                .transpose()?
        } else {
            None
        };
        let run = self.run_prepared_authority_transactions_with_targets(
            prepared,
            authority_transaction_count_for_groups(&authority_groups),
            native_scanout,
            native_head_frames,
            wm_update,
        )?;
        self.last_primary_logical_target = run.primary_logical_target;
        Ok(run.report)
    }

    /// Publishes an ordinary cadence repaint when native ownership permits it.
    /// `None` preserves the caller's repaint obligation for a later cadence;
    /// forced startup and topology repaints use `run_cpu_repaint` directly.
    pub fn run_ordinary_cpu_repaint(
        &mut self,
        scene: &mut LiveProductionCpuScene,
        raised_surface: Option<SurfaceId>,
        focused_surface: Option<SurfaceId>,
        cursor_presentation: LiveProductionCursorPresentation,
        output_descriptors: &[sophia_engine::HeadlessOutput],
        native_scanout: &mut LiveProductionNativeScanout,
    ) -> Result<Option<LiveProductionCpuSubmission>, Box<dyn std::error::Error>> {
        if self.native_publication_blocked() {
            return Ok(None);
        }
        Ok(Some(self.run_cpu_repaint_inner(
            scene,
            raised_surface,
            focused_surface,
            cursor_presentation,
            output_descriptors,
            native_scanout,
        )?))
    }

    pub fn run_cpu_repaint(
        &mut self,
        scene: &mut LiveProductionCpuScene,
        raised_surface: Option<SurfaceId>,
        focused_surface: Option<SurfaceId>,
        cursor_presentation: LiveProductionCursorPresentation,
        output_descriptors: &[sophia_engine::HeadlessOutput],
        native_scanout: &mut LiveProductionNativeScanout,
    ) -> Result<LiveProductionCpuSubmission, Box<dyn std::error::Error>> {
        // Forced startup/topology work cannot be silently deferred. Validate
        // every output before the ordinary helper can transfer any owner.
        if native_scanout
            .outputs()
            .iter()
            .any(|output| native_scanout.output_retirement_protected(output.id))
        {
            return Err("forced repaint waits for an existing distinct retirement".into());
        }
        self.run_cpu_repaint_inner(
            scene,
            raised_surface,
            focused_surface,
            cursor_presentation,
            output_descriptors,
            native_scanout,
        )
    }

    fn run_cpu_repaint_inner(
        &mut self,
        scene: &mut LiveProductionCpuScene,
        raised_surface: Option<SurfaceId>,
        focused_surface: Option<SurfaceId>,
        cursor_presentation: LiveProductionCursorPresentation,
        output_descriptors: &[sophia_engine::HeadlessOutput],
        native_scanout: &mut LiveProductionNativeScanout,
    ) -> Result<LiveProductionCpuSubmission, Box<dyn std::error::Error>> {
        // The same view the retained head frames below are planned from, so a
        // repaint's software list, its chrome observation and its head frames
        // cannot describe three different scenes.
        let committed = self.displayed_surface_view().to_vec();
        let display_list = self.prepare_repaint(&committed, raised_surface, focused_surface)?;
        let output = output_descriptors
            .first()
            .copied()
            .ok_or("software composition has no output descriptor")?;
        let compose_started = Instant::now();
        let composition = scene
            .compose_display_list(
                output,
                &committed,
                &display_list,
                cursor_presentation.composition_position(),
            )?
            .clone();
        self.record_focus_ring_observation(&committed, LiveChromeObservationSource::Repaint, true)?;
        let head_batches = self.retained_output_head_composition_frames(scene, native_scanout)?;
        let output_count = self.outputs.output_count();
        let primary_output = self.outputs.primary_output();
        let production = &self.production;
        let surface_metadata = &self.surface_metadata;
        let ordinary_repaints_pending = &mut self.ordinary_repaints_pending;
        let outputs = &mut self.outputs;
        let mut head_batches = head_batches.into_iter().collect::<BTreeMap<_, _>>();
        let primary_logical_target = std::cell::Cell::new(None);
        let primary_logical_target_ref = &primary_logical_target;
        let mut adapter = crate::LiveProductionOutputRuntimeAdapter::new(
            output_count,
            |index, snapshot: &[CommittedSurfaceState]| -> Result<_, Box<dyn std::error::Error>> {
                let output_id = outputs
                    .output_id(index)
                    .ok_or("production output index was not registered")?;
                let frames = head_batches
                    .remove(&output_id)
                    .ok_or("CPU repaint omitted a logical-output head cohort")?;
                let logical_checksum = frames
                    .first()
                    .map(|frame| frame.logical_content_checksum)
                    .ok_or("CPU repaint produced an empty head cohort")?;
                if frames
                    .iter()
                    .any(|frame| frame.logical_content_checksum != logical_checksum)
                {
                    return Err("CPU repaint heads disagree on logical content checksum".into());
                }
                if outputs.native_initialized(output_id) {
                    let frame = ordinary_repaint::admit(
                        ordinary_repaints_pending,
                        native_scanout,
                        output_id,
                        frames,
                    )?;
                    if Some(output_id) == primary_output {
                        primary_logical_target_ref
                            .set(frame.map(|frame| {
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
                outputs.run_output(index, snapshot, |runtime| {
                    let input = compositor_tick_input_for_committed(
                        snapshot,
                        surface_metadata,
                        0,
                        Vec::new(),
                        None,
                    );
                    Ok(if runtime.rendered_primary_plane_scanout_in_flight() {
                        runtime.run_tick(input)?
                    } else {
                        native_scanout.run_tick(output_id, runtime, input)?
                    })
                })
            },
        );
        let tick = production
            .run_outputs(&mut adapter)?
            .into_iter()
            .next()
            .ok_or("persistent backend runtime has no outputs")?;
        let primary_logical_target = primary_logical_target.get();
        self.last_primary_logical_target = primary_logical_target;
        Ok(LiveProductionCpuSubmission {
            tick,
            composition,
            composed: true,
            compose_elapsed: compose_started.elapsed(),
            primary_logical_target,
        })
    }

    pub fn run_observation_tick(
        &mut self,
    ) -> Result<crate::LiveBackendRuntimeTickReport, Box<dyn std::error::Error>> {
        // Both views from one read, and the assembly resynchronised before the
        // tick. This was the one tick that never replaced the committed list, so
        // it paired fresh templates against whatever an earlier cycle had left in
        // the assembly -- a mismatch the engine rejects as an invalid surface,
        // masked until the first client surface ever committed and deterministic
        // from then on. Nine call paths lead here, which is why the failure
        // looked unrelated to any of them.
        let (layer_templates, committed) = self.scene_views();
        let output = self
            .outputs
            .values_mut()
            .next()
            .ok_or("persistent backend runtime has no outputs")?;
        output
            .runtime
            .assembly_mut()
            .replace_committed_surfaces(committed);
        Ok(output
            .runtime
            .run_tick(compositor_tick_input(&layer_templates, 0, Vec::new(), None))?)
    }
}
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
