use crate::*;
use sophia_engine::{CompositorBackendTickInput, OutputFramePresentationState};
use sophia_protocol::{OutputId, TransactionId};
use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

mod composition_admission;
mod composition_installation;
mod mirror_completion;
mod presentation_timing;
#[cfg(test)]
pub(crate) use presentation_timing::{PresentedTimingHead, completed_timing};
mod settled_mirror;
#[cfg(any(test, feature = "test-support"))]
pub(crate) use composition_admission::{NativeCompositionOutput, prepare_native_composition_batch};
#[cfg(any(test, feature = "test-support"))]
pub(crate) use composition_installation::{
    CompositionInstallation, CompositionInstaller, install_composition_generation,
    reserve_composition_lifecycle,
};
#[cfg(any(test, feature = "test-support"))]
pub(crate) use composition_installation::{
    NativeCompositionInstallationHead, validate_composition_installation,
};
#[cfg(any(test, feature = "test-support"))]
pub(crate) use composition_queue::DeferredNativeCompositions;
#[cfg(any(test, feature = "test-support"))]
pub(crate) use composition_queue::LiveProductionQueuedMirrorHeadFrame;
#[cfg(any(test, feature = "test-support"))]
pub(crate) use mirror_completion::{MirrorCompletionWitness, complete_mirror_head};
#[cfg(any(test, feature = "test-support"))]
pub(crate) use renderer_images::LiveProductionHeadCompositionContent;
#[cfg(any(test, feature = "test-support"))]
pub(crate) use settled_mirror::{SettledMirrorHead, settled_mirror_checksum};
mod composition_queue;
mod cursor;
mod frame_damage;
mod layout_probe;
mod layout_retirement;
pub(crate) use layout_retirement::LiveProductionNativeRetirementContent;
pub use layout_retirement::LiveProductionRetiredLayoutWitness;
mod output_capabilities;
mod render_devices;
pub use render_devices::{
    LiveOutputAllocationContext, LiveOutputAllocationFormatPreference,
    LiveOutputAllocationPreference, LiveRenderDeviceNodeIdentity,
};
mod renderer_handoff;
mod renderer_images;
mod shutdown;
mod state;
mod topology;
pub use cursor::project_native_cursor_logical_viewport;
pub use frame_damage::project_mirror_output_damage_snapshot;
use frame_damage::{
    trace_native_head_retirement, trace_presented_mirror_head_damage, trace_presented_output_damage,
};
pub use renderer_handoff::LiveProductionRendererImageHandoff;
pub use renderer_images::{
    LiveProductionHeadCompositionFrame, live_topology_frame_renderer_image_requirements,
    validate_live_head_composition_frame_batch,
};
pub use state::*;
pub use topology::*;

pub struct LiveProductionNativeScanout {
    /// Submit-to-flip samples; the offer-to-submit half lives per
    /// exporter, and `direct_scanout_cost` merges the two.
    cost: crate::DirectScanoutCost,
    pub groups: Vec<LiveProductionNativeGroup>,
    pub heads: Vec<LiveProductionNativeHead>,
    /// Logical output descriptors are independent of physical head extents.
    /// A mirrored head keeps its native size in `head.output`, while this
    /// table is what Engine/session policy publishes.
    logical_outputs: Vec<sophia_engine::HeadlessOutput>,
    pub discovered_outputs: usize,
    pub presentation_outputs: usize,
    pub submissions: usize,
    pub submit_deferred: usize,
    pub submit_failures: usize,
    pub retirements: usize,
    pub retire_failures: usize,
    pub max_in_flight_ticks: u64,
    /// The most KMS submissions this output ever had in flight at once.
    ///
    /// `max_in_flight_ticks` measures how *long* a submission was in
    /// flight, which cannot tell one long submission from two overlapping
    /// ones. This measures depth, so the one-submission rule becomes
    /// evidence instead of a claim. A mirror output holds one per head by
    /// design, so the bound this proves is per head rather than per output.
    pub max_in_flight_per_output: usize,
    /// Frames the latest-wins pending cell dropped without rendering.
    pub pending_frame_supersessions: usize,
    /// The most renders siblings completed while one head waited. Zero
    /// when heads never wait on each other, which is every session in
    /// which they do not share a renderer thread.
    pub max_service_skew: usize,
    /// Whether the session asked for direct scanout at all.
    pub direct_scanout_admissible: bool,
    translation_motion_active: bool,
    /// Whether startup readiness has proven a picture reached glass. Until
    /// it has, every head composes: the proof reads composed pixels, and a
    /// direct frame produces none.
    pub direct_scanout_admitted: bool,
    pub max_submit_to_page_flip: Duration,
    pub callback_accepted: usize,
    pub callback_rejected: usize,
    pub callback_queue_saturated: usize,
    pub nonzero_exports: usize,
    /// One scanout buffer exporter per head, parallel to `heads`.
    ///
    /// Per head because each connector scans out its own buffer at its own
    /// mode. A group's heads show one *scene*, not one buffer: sharing a buffer
    /// would force every head onto a single mode, which is the design this
    /// replaced -- it could not mirror displays of different resolutions
    /// without degrading the better one.
    ///
    /// `LiveProductionNativeGroup` is a *card session*, not a mirror group. The
    /// exporter belongs to neither: it belongs to a head.
    exporters: Vec<
        crate::NativeGbmRenderedScanoutBufferDiscoveryExporter<
            crate::RealAtomicScanoutRenderDeviceDiscovery,
        >,
    >,
    /// Seat-admitted render nodes retained for this native owner's lifetime.
    image_import_devices: Vec<std::fs::File>,
    render_devices: render_devices::LiveRenderDeviceState,
    /// Primary presentation and last-head ownership for mirror generations.
    output_lifecycles: BTreeMap<OutputId, LiveProductionMirrorGroupLifecycle>,
    /// Engine-owned prepare/submit/flip barrier for the active generation
    /// of each multi-head logical output.
    output_cohorts:
        BTreeMap<(OutputId, LiveProductionNativeFrameId), sophia_engine::OutputPresentationCohort>,
    /// Latest ordinary successor held behind a Present generation until
    /// the primary head owns that Present in KMS.
    deferred_mirror_generations: composition_queue::DeferredNativeCompositions,
    /// Candidate and rollback owners for one live output-topology effect.
    /// Ordinary frame scheduling is quarantined while this is present.
    output_topology_preparation: Option<LiveProductionNativeTopologyPreparation>,
    /// Cleanup owners may outnumber physical heads when candidate and
    /// rollback pools are cancelled together, so they cannot share the
    /// ordinary one-slot-per-head cleanup ledger.
    output_topology_cleanup: Vec<(
        sophia_engine::RenderHeadId,
        crate::BoxedRenderedPrimaryPlaneScanoutCleanup,
    )>,
    /// The only place a head's card, connector, and CRTC identity lives.
    pub head_table: crate::LiveProductionNativeHeadTable,
    native_frame_owner: crate::NativeFrameOwner,
    next_frame_id: u64,
    next_head_candidate_id: u64,
    pub production_page_flips: crate::LiveProductionPageFlipTracker,
    pub kernel_page_flip_timestamps: usize,
    pub kernel_page_flip_timestamp_missing: usize,
    kernel_page_flip_ust: BTreeMap<(OutputId, sophia_engine::RenderHeadId, u64), u64>,
    pub vsync_overlap_rejections: usize,
    pub page_flip_phase_rejections: usize,
    pub cursor_updates: usize,
    pub cursor_hidden_updates: usize,
    /// Latest-wins atomic positions accepted while a head was busy.
    pub cursor_updates_queued: usize,
    /// Pending positions replaced before the plane could show them.
    pub cursor_updates_coalesced: usize,
    /// Atomic cursor updates carried by a primary-plane commit.
    pub cursor_updates_ridden: usize,
    /// Atomic cursor-only commits made while primary content was idle.
    pub cursor_only_commits: usize,
    /// Longest one of those blocking commits, and their total. They wait
    /// for the kernel to apply them at a vblank, so this is owner-loop
    /// time a frame could not use. The quiet gate exists to keep both
    /// near zero while a client is drawing.
    pub max_cursor_only_commit: Duration,
    pub cursor_only_commit_total: Duration,
    /// Combined primary/cursor requests retried as cursor-only commits.
    pub cursor_combined_drops: usize,
    /// Runtime atomic cursor rejection transitions to the legacy ioctl.
    pub cursor_legacy_fallbacks: usize,
    pub cursor_initialization_deferrals: usize,
    /// Legacy-ioctl cursor updates issued while a page flip was in
    /// flight, which an ioctl may do and an atomic commit may not. Named
    /// for the path that counts it: the atomic path returns before this
    /// is reached, so a session that later took the cursor plane still
    /// carries whatever it accumulated beforehand.
    pub legacy_cursor_updates_primary_in_flight: usize,
    /// Which cursor path this session is driving, and what the card said
    /// it would accept. Two facts, kept apart: a session can be on the
    /// legacy ioctl while the card would happily scan a cursor plane,
    /// and a record that reported one as the other would be describing a
    /// capability as a decision.
    pub cursor_path: crate::HardwareCursorPath,
    pub cursor_update_failures: usize,
    pub max_cursor_initialization: Duration,
    pub max_cursor_update: Duration,
    /// Oldest accepted motion-to-plane completion observed by the backend.
    pub max_cursor_queue_delay: Duration,
}

pub struct LiveProductionNativeGroup {
    pub session: crate::RealAtomicScanoutPageFlipSession,
    /// Topology-sized storage reused by the card completion pump.
    /// The owner drains this before any watchdog can inspect a head.
    pub callbacks: Vec<crate::LivePageFlipCallback>,
    /// Kernel timing stays separate because an out-fence completion has no
    /// kernel vblank timestamp.
    pub timestamps: Vec<crate::LibdrmKernelPageFlipTimestamp>,
    /// The renderer thread every head on this card shares, once sharing is
    /// on. A group is a card session, which is exactly the DRM device
    /// group the heads render against: one EGL display, one GBM device,
    /// and one renderer-image store for all of them.
    pub renderer_core: Option<std::sync::Arc<crate::NativeGbmRendererWorkerCore>>,
}

struct LiveProductionMirrorRetirementReport {
    page_flip_callbacks: crate::LivePageFlipCallbackQueueReport,
    completed_retire: Option<crate::LiveTrackedRenderedPrimaryPlaneScanoutRetireReport>,
    completed_serial: Option<u64>,
    errors: Vec<String>,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveProductionKmsCompletionMode {
    PageFlipPreferred,
    OutFenceAuthoritative,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveProductionKmsCompletionSource {
    PageFlipEvent,
    OutFence,
}

impl LiveProductionKmsCompletionSource {
    const fn label(self) -> &'static str {
        match self {
            Self::PageFlipEvent => "page_flip_event",
            Self::OutFence => "out_fence",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LiveProductionCompletionTimestamp {
    pub ust_usec: u64,
    pub used_kernel_timestamp: bool,
    pub missing_kernel_timestamp: bool,
}

pub const fn reduce_live_production_completion_timestamp(
    source: LiveProductionKmsCompletionSource,
    kernel_ust_usec: Option<u64>,
    monotonic_fallback_ust_usec: u64,
) -> LiveProductionCompletionTimestamp {
    match (source, kernel_ust_usec) {
        (LiveProductionKmsCompletionSource::PageFlipEvent, Some(ust_usec)) => {
            LiveProductionCompletionTimestamp {
                ust_usec,
                used_kernel_timestamp: true,
                missing_kernel_timestamp: false,
            }
        }
        (LiveProductionKmsCompletionSource::PageFlipEvent, None) => {
            LiveProductionCompletionTimestamp {
                ust_usec: monotonic_fallback_ust_usec,
                used_kernel_timestamp: false,
                missing_kernel_timestamp: true,
            }
        }
        (LiveProductionKmsCompletionSource::OutFence, _) => LiveProductionCompletionTimestamp {
            ust_usec: monotonic_fallback_ust_usec,
            used_kernel_timestamp: false,
            missing_kernel_timestamp: false,
        },
    }
}

pub struct LiveProductionNativeHead {
    pub head: sophia_engine::RenderHeadId,
    pub enabled: bool,
    pub group: usize,
    pub selection: crate::LibdrmNativePrimaryPlaneSelection,
    format_capabilities: crate::LibdrmNativePlaneFormatCapabilities,
    /// Where the cursor should be on this head, not yet committed.
    ///
    /// A cell, not a queue: latest wins and supersedes in place. A
    /// backlog that grew per pointer event would be unbounded by
    /// construction, which is what `CursorWorkBoundedByAvailability`
    /// forbids -- a hand moving a mouse produces motion far faster than a
    /// display retires frames.
    ///
    /// `None` means nothing is waiting. The pointer being on another head
    /// is a *placement* of `None` inside `Some`, which is how a head is
    /// told to hide rather than told nothing.
    pub pending_cursor: Option<Option<crate::LibdrmNativeCursorPlacement>>,
    /// When the current pending cell first became nonempty.
    ///
    /// Superseding preserves the timestamp: the bound describes how long
    /// the plane went without reaching an accepted desired state, not how
    /// recently the newest mouse packet arrived.
    pub pending_cursor_since: Option<Instant>,
    /// What this head is currently showing, so a redundant commit can be
    /// skipped and a ghost can be noticed.
    pub committed_cursor: Option<crate::LibdrmNativeCursorPlacement>,
    /// This head's cursor plane properties, discovered once.
    ///
    /// `None` until asked, and still `None` afterwards if the card has no
    /// cursor plane for this CRTC or its plane cannot be positioned --
    /// both of which mean the head keeps the legacy ioctl.
    pub cursor_properties: Option<crate::LibdrmNativeCursorPlanePropertyHandles>,
    /// The placement a prepared-but-unsubmitted commit is carrying.
    ///
    /// Mirror heads prepare in one pass and submit in a later one, so the
    /// value armed at prepare time has to survive to the accept -- and
    /// settle with what was actually aboard the request, not whatever is
    /// pending by then.
    pub prepared_cursor_ride: Option<Option<crate::LibdrmNativeCursorPlacement>>,
    pub scale: u32,
    pub refresh_millihz: u32,
    pub transform: sophia_protocol::OutputTransform,
    pub mapping: sophia_protocol::OutputHeadMapping,
    pub vrr: sophia_protocol::OutputVrrPolicy,
    /// One KMS submission may be outstanding per head, so one decoded
    /// completion may wait for that owner. A second live completion is a
    /// terminal saturation error, never a discard.
    pub pending_callback: Option<crate::LivePageFlipCallback>,
    pub completion_mode: LiveProductionKmsCompletionMode,
    pub completion_fence_status: crate::LibdrmNativeCompletionFenceStatus,
    pub out_fence_retirements: usize,
    pub late_page_flip_events: usize,
    pub completion_fence_errors: usize,
    pub output: sophia_engine::HeadlessOutput,
    pub target_generation: u64,
    pub submitted_at: Option<Instant>,
    pub submitted_ust_usec: Option<u64>,
    pub pending_nonzero_pixel_bytes: usize,
    pub last_checksum: u64,
    pub submitted_checksum: Option<u64>,
    pub submitted_sequence: Option<usize>,
    pub pending_content: Option<LiveProductionScanoutContent>,
    pub rendering_content: Option<LiveProductionScanoutContent>,
    pub submitted_content: Option<LiveProductionScanoutContent>,
    /// Whether the submission in flight put the client's own buffer on the
    /// plane rather than a compositor copy.
    ///
    /// It decides how the Present settles: a copy is idle at the flip, but
    /// a directly scanned buffer is on glass and stays owed to the client
    /// until a successor flip retires it.
    /// See `PresentFlipOwnership.tla`.
    pub submitted_direct: bool,
    layout_witness: layout_retirement::NativeLayoutWitnessState,
    /// The same, for the submission the screen is now showing.
    pub presented_direct: bool,
    pub presented_content: Option<LiveProductionScanoutContent>,
    /// Checksum of the logical scene this head presented, never of the pixels
    /// this head scanned out. A mirror group composes one scene and projects
    /// it into each head's own mode, so head pixels legitimately differ while
    /// this value must not: the group join below refuses heads that disagree
    /// on it, and comparing per-head pixels there would refuse every mirror
    /// whose heads differ in size. Anything head-local belongs in a separate
    /// field, not here.
    pub presented_logical_checksum: u64,
    pub presented_submissions: usize,
    pub presented_submission_ust_usec: u64,
    pub presented_page_flip_ust_usec: u64,
    pub presented_completion_timestamp: Option<LiveProductionCompletionTimestamp>,
    pub presented_submit_to_page_flip: Duration,
    /// Sibling completions when this head's current request went
    /// outstanding, or `None` while it has nothing in flight.
    pub(crate) service_skew_baseline: Option<usize>,
    pub submissions: usize,
    pub retirements: usize,
    pub callback_accepted: usize,
    pub initial_modeset_submission: Option<usize>,
    pub nonzero_exports: usize,
    pub last_submit_report: Option<crate::LiveTrackedRenderedPrimaryPlaneScanoutSubmitReport>,
    pub output_frames: OutputFramePresentationState,
    /// This physical head's synchronously displayed baseline. Single-head
    /// outputs keep that owner in the logical runtime; mirror groups transfer
    /// every connector's owner here after initialization.
    pub(crate) scanout_custody: crate::PersistentScanoutCustody,
    pub(crate) displayed_group_frame: Option<LiveProductionNativeFrameId>,
    pub(crate) prepared_scanout: Option<
        crate::LivePreparedRenderedPrimaryPlaneScanout<crate::NativeGbmRenderedScanoutOwner>,
    >,
    pub(crate) prepared_group_frame: Option<LiveProductionNativeFrameId>,
    pub(crate) prepared_worker_was_in_flight: bool,
    pub(crate) scanout_in_flight_ticks: u64,
    pub(crate) last_callback_serial: Option<u64>,
    pub(crate) submitted_group_frame: Option<LiveProductionNativeFrameId>,
}

fn mirror_tracked_prepare_report(
    prepare: &crate::LiveRenderedPrimaryPlaneScanoutPrepareResult<
        crate::NativeGbmRenderedScanoutOwner,
    >,
    size: sophia_protocol::Size,
) -> crate::LiveTrackedRenderedPrimaryPlaneScanoutSubmitReport {
    use crate::LiveRenderedPrimaryPlaneScanoutPrepareStatus as Prepare;
    use crate::LiveTrackedRenderedPrimaryPlaneScanoutSubmitStatus as Tracked;
    let (status, runtime_scanout_state) = match prepare.status {
        Prepare::Prepared => (
            Tracked::ScanoutExportPending,
            crate::RuntimeScanoutState::Deferred,
        ),
        Prepare::ScanoutExportPending => (
            Tracked::ScanoutExportPending,
            crate::RuntimeScanoutState::Deferred,
        ),
        Prepare::ScanoutTargetNotReady => (
            Tracked::ScanoutTargetNotReady,
            crate::RuntimeScanoutState::Rejected,
        ),
        Prepare::FrameTargetUnavailable => (
            Tracked::FrameTargetUnavailable,
            crate::RuntimeScanoutState::Rejected,
        ),
        Prepare::ScanoutExportFailed => (
            Tracked::ScanoutExportFailed,
            crate::RuntimeScanoutState::Rejected,
        ),
        Prepare::PrimaryPlanePrepareFailed => (
            Tracked::PrimaryPlaneSubmitFailed,
            crate::RuntimeScanoutState::Rejected,
        ),
    };
    crate::LiveTrackedRenderedPrimaryPlaneScanoutSubmitReport {
        status,
        layout_witness: None,
        scanout_target: prepare.scanout_target,
        output_size: Some(size),
        target: prepare.target,
        target_size: Some(size),
        export: prepare.export,
        scanout_buffer: prepare.scanout_buffer,
        buffer_format: prepare.buffer_format,
        buffer_modifier: prepare.buffer_modifier,
        buffer_planes: prepare.buffer_planes,
        properties: prepare.properties,
        format_table: prepare.format_table,
        resources: prepare.resources,
        framebuffer: prepare.framebuffer,
        request: prepare.request,
        submit: prepare.submit,
        request_scope: prepare.request_scope,
        commit_flags: prepare.commit_flags,
        commit_submit: None,
        atomic_test: None,
        runtime_scanout_state: Some(runtime_scanout_state),
        in_flight: false,
        in_flight_ticks: 0,
        cleanup_pending: prepare.cleanup.is_some(),
        cursor_dropped: false,
    }
}

fn mirror_tracked_submit_report(
    result: &crate::LiveRenderedPrimaryPlaneScanoutSubmitResult<
        crate::NativeGbmRenderedScanoutOwner,
    >,
    size: sophia_protocol::Size,
) -> crate::LiveTrackedRenderedPrimaryPlaneScanoutSubmitReport {
    crate::LiveTrackedRenderedPrimaryPlaneScanoutSubmitReport {
        status: result.status.into(),
        layout_witness: None,
        scanout_target: result.scanout_target,
        output_size: Some(size),
        target: result.target,
        target_size: Some(size),
        export: result.export,
        scanout_buffer: result.scanout_buffer,
        buffer_format: result.buffer_format,
        buffer_modifier: result.buffer_modifier,
        buffer_planes: result.buffer_planes,
        properties: result.properties,
        format_table: result.format_table,
        resources: result.resources,
        framebuffer: result.framebuffer,
        request: result.request,
        submit: result.submit,
        request_scope: result.request_scope,
        commit_flags: result.commit_flags,
        commit_submit: result.commit_submit,
        atomic_test: result.atomic_test,
        runtime_scanout_state: Some(result.runtime_scanout_state()),
        in_flight: result.submission.is_some(),
        in_flight_ticks: 0,
        cleanup_pending: result.cleanup.is_some(),
        cursor_dropped: result.cursor_dropped,
    }
}

/// What the direct scanout path did, across every head of a session.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LiveProductionDirectScanoutTotals {
    /// Frames Engine proved and this session tried to scan out directly.
    pub attempts: usize,
    /// Frames whose client buffer the driver accepted onto a plane.
    pub flips: usize,
    /// Validating `TEST_ONLY` commits issued on an eligibility edge.
    pub tests: usize,
    /// Validating commits the driver refused. Each ends an episode.
    pub test_rejections: usize,
    /// Proven frames the backend's own re-derivation disagreed with.
    /// Nonzero means Engine and the lowered pixels disagree, which is a
    /// defect rather than ordinary ineligibility -- an ineligible frame
    /// never becomes an attempt at all.
    pub refusals: usize,
    /// Proven frames the backend declined for a reason of its own: a
    /// format or plane layout it cannot use. Engine proves structure and
    /// never looks at a pixel format, so this is the backend answering a
    /// question Engine did not ask, and it is not a defect.
    pub unsupported: usize,
    /// Direct attempts that composed instead, having reached no screen.
    pub fallbacks: usize,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LivePersistentRenderMetrics {
    pub target_creations: usize,
    pub target_recreations: usize,
    pub pipeline_creations: usize,
    pub frame_surface_creations: usize,
    pub cpu_target_creations: usize,
    pub dmabuf_target_creations: usize,
    pub composition_target_creations: usize,
    pub composition_target_reuses: usize,
    pub generation_replacements: usize,
    pub recovery_replacements: usize,
    pub uploads: usize,
    pub snapshot_captures: usize,
    pub snapshot_promotions: usize,
    pub snapshot_rollbacks: usize,
    pub snapshot_evictions: usize,
    pub snapshot_live_entries: usize,
    pub snapshot_live_bytes: u64,
    pub import_cache_imports: usize,
    pub import_cache_hits: usize,
    pub import_cache_evictions: usize,
    pub import_cache_live_entries: usize,
    pub import_cache_descriptor_mismatches: usize,
    pub import_cache_capacity_rejections: usize,
    pub exact_nearest_draws: usize,
    pub sharp_downscale_draws: usize,
    pub sharp_upscale_draws: usize,
    pub linear_fallback_draws: usize,
    pub worker_requests: usize,
    pub worker_completions: usize,
    pub worker_failures: usize,
    pub worker_soft_stalls: usize,
    pub worker_hard_stalls: usize,
    pub worker_release_enqueue_failures: usize,
    /// Renderer threads this session ran: one per card group when outputs
    /// share, one per enabled head when they do not. The difference the
    /// coalescing row exists to make, and invisible in every other
    /// counter.
    pub renderer_workers: usize,
    /// Results that reached an output naming a different one. Zero by
    /// construction; reported so the claim is checked rather than assumed.
    pub worker_result_misroutes: usize,
    pub frame_slot_acquisitions: usize,
    pub frame_slot_reuses: usize,
    pub frame_slot_deferrals: usize,
    pub frame_slot_stale_releases: usize,
    pub frame_slots_leased: usize,
    pub frame_slots_high_watermark: usize,
    pub frame_slot_partial_repaints: usize,
    pub frame_slot_full_repaints: usize,
    pub frame_slot_history_invalidations: usize,
    pub frame_slot_history_records: usize,
    pub max_worker_request: Duration,
    pub max_target_create: Duration,
    pub max_frame_surface_create: Duration,
    pub max_render: Duration,
    pub max_upload: Duration,
}

include!("persistent_native_scanout/construction.rs");
include!("persistent_native_scanout/head_access.rs");
include!("persistent_native_scanout/page_flip_watchdog.rs");
include!("persistent_native_scanout/render_access.rs");
include!("persistent_native_scanout/singleton_tick.rs");
include!("persistent_native_scanout/in_flight_observation.rs");
include!("persistent_native_scanout/mirror_retirement.rs");
include!("persistent_native_scanout/mirror_scene_tick.rs");
include!("persistent_native_scanout/frame_retirement.rs");
include!("persistent_native_scanout/completion_callbacks.rs");
include!("persistent_native_scanout/renderer_initialization.rs");
include!("persistent_native_scanout/direct_scanout_observation.rs");
include!("persistent_native_scanout/frame_queue.rs");
include!("persistent_native_scanout/frame_observation.rs");
include!("persistent_native_scanout/completion_pump.rs");

fn trace_live_native_lifecycle(stage: &str) {
    if std::env::var_os("SOPHIA_LIVE_SESSION_DIAGNOSTIC").is_some() {
        tracing::info!("sophia_live_native_lifecycle schema=1 stage={stage}");
    }
}
