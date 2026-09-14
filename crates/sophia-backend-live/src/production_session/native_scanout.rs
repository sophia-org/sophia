#[cfg(all(feature = "libdrm-events", feature = "gbm-probe"))]
mod persistent_native_scanout {
    use crate::*;
    use sophia_engine::{CompositorBackendTickInput, OutputFramePresentationState};
    use sophia_protocol::{OutputId, TransactionId};
    use std::collections::{BTreeMap, BTreeSet};
    use std::time::{Duration, Instant};

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
    mod state;
    mod topology;
    pub use cursor::project_native_cursor_logical_viewport;
    pub use frame_damage::project_mirror_output_damage_snapshot;
    use frame_damage::{
        trace_native_head_retirement, trace_presented_mirror_head_damage,
        trace_presented_output_damage,
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
        output_cohorts: BTreeMap<
            (OutputId, LiveProductionNativeFrameId),
            sophia_engine::OutputPresentationCohort,
        >,
        /// Latest ordinary successor held behind a Present generation until
        /// the primary head owns that Present in KMS.
        deferred_mirror_generations:
            BTreeMap<OutputId, renderer_images::LiveProductionQueuedMirrorGeneration>,
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
        /// Combined primary/cursor requests retried as cursor-only commits.
        pub cursor_combined_drops: usize,
        /// Runtime atomic cursor rejection transitions to the legacy ioctl.
        pub cursor_legacy_fallbacks: usize,
        pub cursor_initialization_deferrals: usize,
        pub cursor_updates_primary_in_flight: usize,
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
        pub(crate) displayed_scanout: Option<crate::BoxedRenderedPrimaryPlaneScanoutSubmission>,
        pub(crate) displayed_group_frame: Option<LiveProductionNativeFrameId>,
        pub(crate) scanout_submission: Option<crate::BoxedRenderedPrimaryPlaneScanoutSubmission>,
        pub(crate) prepared_scanout: Option<
            crate::LivePreparedRenderedPrimaryPlaneScanout<crate::NativeGbmRenderedScanoutOwner>,
        >,
        pub(crate) prepared_group_frame: Option<LiveProductionNativeFrameId>,
        pub(crate) prepared_worker_was_in_flight: bool,
        pub(crate) scanout_cleanup: Option<crate::BoxedRenderedPrimaryPlaneScanoutCleanup>,
        pub(crate) scanout_cleanup_group_frame: Option<LiveProductionNativeFrameId>,
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

    impl LiveProductionNativeScanout {
        pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
            Self::new_with_mirroring(&crate::NativeMirrorGrouping::none())
        }

        /// Builds without a seat, with connectors grouped into logical outputs.
        ///
        /// The standalone topology commands need this: they read the operator's
        /// profile to reconcile against, so building the card set without the
        /// grouping that profile asks for would validate a different desktop than
        /// the one configured -- two outputs where the operator asked for one.
        pub fn new_with_mirroring(
            grouping: &crate::NativeMirrorGrouping,
        ) -> Result<Self, Box<dyn std::error::Error>> {
            Self::new_with_selection(
                crate::select_real_atomic_scanout_cards(),
                grouping,
                sophia_protocol::OutputHeadMapping::Fit,
            )
        }

        #[cfg(feature = "seat-control")]
        pub fn new_with_seat(
            opener: &crate::LiveSeatDeviceOpener,
        ) -> Result<Self, Box<dyn std::error::Error>> {
            Self::new_with_seat_and_mirroring(opener, &crate::NativeMirrorGrouping::none())
        }

        /// Builds the scanout with connectors grouped into logical outputs.
        ///
        /// The grouping comes from configuration and is the only thing that makes
        /// mirroring happen: without it every connector is its own logical output,
        /// which is the ordinary desktop and was the only shape reachable before.
        #[cfg(feature = "seat-control")]
        pub fn new_with_seat_and_mirroring(
            opener: &crate::LiveSeatDeviceOpener,
            grouping: &crate::NativeMirrorGrouping,
        ) -> Result<Self, Box<dyn std::error::Error>> {
            Self::new_with_seat_mirroring_and_mapping(
                opener,
                grouping,
                sophia_protocol::OutputHeadMapping::Fit,
            )
        }

        /// Builds a scanout whose initial physical heads retain the neutral
        /// mapping selected by configuration. Later output-authority topology
        /// commits replace that value independently per head.
        #[cfg(feature = "seat-control")]
        pub fn new_with_seat_mirroring_and_mapping(
            opener: &crate::LiveSeatDeviceOpener,
            grouping: &crate::NativeMirrorGrouping,
            mapping: sophia_protocol::OutputHeadMapping,
        ) -> Result<Self, Box<dyn std::error::Error>> {
            let mut scanout = Self::new_with_selection(
                crate::select_real_atomic_scanout_cards_with_seat(opener),
                grouping,
                mapping,
            )?;
            #[cfg(feature = "drm-hotplug")]
            let image_import_devices = match crate::discover_seat_render_devices(opener.name()) {
                Ok(devices) => devices.into_iter().map(|device| device.file).collect(),
                Err(reason) => {
                    tracing::warn!(
                        ?reason,
                        "renderer image import device inventory unavailable"
                    );
                    Vec::new()
                }
            };
            #[cfg(not(feature = "drm-hotplug"))]
            let image_import_devices = Vec::new();
            scanout.image_import_devices = image_import_devices;
            scanout.refresh_allocation_devices();
            Ok(scanout)
        }

        /// Builds the native owner with one already-resolved compositor cursor.
        /// The backend receives pixels, never a theme name or styling policy.
        #[cfg(feature = "seat-control")]
        pub fn new_with_seat_mirroring_mapping_and_cursor(
            opener: &crate::LiveSeatDeviceOpener,
            grouping: &crate::NativeMirrorGrouping,
            mapping: sophia_protocol::OutputHeadMapping,
            cursor: sophia_engine::CursorAsset,
        ) -> Result<Self, Box<dyn std::error::Error>> {
            let mut scanout = Self::new_with_seat_mirroring_and_mapping(opener, grouping, mapping)?;
            for group in &mut scanout.groups {
                group.session.set_hardware_cursor_asset(cursor.clone())?;
            }
            Ok(scanout)
        }

        /// Repaints every head's cursor with a new asset.
        ///
        /// Each group scans out its own cursor buffer, so all of them are
        /// repainted or the pointer would change appearance depending on which
        /// display it happened to be over. A group that refuses leaves the
        /// earlier ones already repainted; that is a cursor drawn at two sizes
        /// across two monitors for as long as it takes the caller to ask for
        /// the old one back, which is worth strictly less than the alternative
        /// of never being able to change it at all.
        #[cfg(feature = "seat-control")]
        pub fn replace_hardware_cursor_asset(
            &mut self,
            cursor: sophia_engine::CursorAsset,
        ) -> Result<(), Box<dyn std::error::Error>> {
            for group in &mut self.groups {
                group.session.set_hardware_cursor_asset(cursor.clone())?;
            }
            Ok(())
        }

        /// Whether every head could hold a cursor of this size.
        ///
        /// All of them, because a cursor that only some displays can show is
        /// not a cursor the session can offer.
        #[cfg(feature = "seat-control")]
        pub fn hardware_cursor_admits_size(&self, width: u32, height: u32) -> bool {
            self.groups
                .iter()
                .all(|group| group.session.hardware_cursor_admits_size(width, height))
        }

        fn new_with_selection(
            selection: crate::RealAtomicScanoutSelectionSet,
            grouping: &crate::NativeMirrorGrouping,
            initial_mapping: sophia_protocol::OutputHeadMapping,
        ) -> Result<Self, Box<dyn std::error::Error>> {
            let authority = crate::RealAtomicScanoutSmokeConfig::default_primary_output()
                .ok_or("persistent native scanout config is invalid")?
                .authority;
            let mut sessions =
                selection.into_page_flip_sessions_with_mirroring(authority, grouping);
            if sessions.status != crate::RealAtomicScanoutPageFlipSessionSetStatus::Ready {
                return Err(format!(
                    "persistent native scanout could not open all KMS outputs: {:?}",
                    sessions.status
                )
                .into());
            }
            let connector_records = crate::discover_native_connector_records("/sys/class/drm")?;
            // Ownership is complete when every discovered connector has a head, not
            // when the logical-output count matches. A mirror group is several heads
            // behind one logical output, so comparing logical outputs to connectors
            // would call a correctly mirrored desktop partial.
            let head_count: usize = sessions
                .sessions
                .iter()
                .map(|session| session.selections().len())
                .sum();
            if head_count != connector_records.len() {
                return Err(format!(
                    "persistent native ownership is partial: discovered={} heads={}",
                    connector_records.len(),
                    head_count
                )
                .into());
            }
            let head_table =
                crate::LiveProductionNativeHeadTable::from_records(sessions.head_records.clone())?;
            let mut presentation_outputs = sophia_engine::EngineHeadRegistry::new();
            for session in &sessions.sessions {
                for ((selection, output_id), head_id) in session
                    .selections()
                    .iter()
                    .copied()
                    .zip(session.outputs().iter().copied())
                    .zip(session.heads().iter().copied())
                {
                    let Some(record) = connector_records
                        .iter()
                        .find(|record| record.connector_id == selection.connector_id())
                    else {
                        return Err(format!(
                            "persistent native output has no Engine connector match: connector={}",
                            selection.connector_id(),
                        )
                        .into());
                    };
                    let target = sophia_engine::HeadRenderTarget {
                        head: head_id,
                        output: output_id,
                        target_generation: 1,
                        native_size: selection.size(),
                        scale: record.scale,
                        refresh_millihz: record.mode.refresh_millihz,
                        transform: sophia_protocol::OutputTransform::Normal,
                        mapping: initial_mapping,
                    };
                    if !presentation_outputs.admit(target).is_admitted() {
                        return Err(format!(
                            "persistent native head admission failed: head={} output={}",
                            head_id.raw(),
                            output_id.raw(),
                        )
                        .into());
                    }
                }
            }
            for record in &sessions.head_records {
                if grouping.is_group_primary(&record.connector_name)
                    && presentation_outputs.set_primary_head(record.output, record.head)
                        != sophia_engine::EngineLogicalOutputUpdate::Updated
                {
                    return Err(format!(
                        "configured mirror primary is not an admitted head: head={} output={}",
                        record.head.raw(),
                        record.output.raw(),
                    )
                    .into());
                }
            }
            if presentation_outputs.output_count() != sessions.output_count {
                return Err(format!(
                    "persistent native connector mapping is incomplete: mapped={} native={}",
                    presentation_outputs.output_count(),
                    sessions.output_count,
                )
                .into());
            }
            let presentation_output_count = presentation_outputs.output_count();
            let production_page_flips =
                crate::LiveProductionPageFlipTracker::from_outputs(&presentation_outputs);
            let mut groups = Vec::new();
            let mut heads = Vec::new();
            let mut exporters = Vec::new();
            for session in sessions.sessions.drain(..) {
                let group = groups.len();
                for ((selection, output_id), head_id) in session
                    .selections()
                    .iter()
                    .copied()
                    .zip(session.outputs().iter().copied())
                    .zip(session.heads().to_vec())
                {
                    let size = selection.size();
                    let target = *presentation_outputs
                        .head(head_id)
                        .expect("native head was admitted before owner construction");
                    // This head's own exporter, against this head's own plane
                    // formats. The group-wide modifier intersection went with the
                    // shared buffer that needed it: a head scanning out its own
                    // buffer is constrained only by its own plane.
                    let discovery = session.render_device_discovery()?;
                    let formats = session.scanout_format_capabilities_for_selection(selection);
                    let modifiers = formats.preferred_xrgb8888_modifiers.clone();
                    let snapshot = formats.snapshot.clone();
                    exporters.push(
                        crate::NativeGbmRenderedScanoutBufferDiscoveryExporter::new(discovery)
                            .with_preferred_modifiers(modifiers)
                            .with_layout_probe_formats(snapshot),
                    );
                    heads.push(LiveProductionNativeHead {
                        head: head_id,
                        enabled: true,
                        group,
                        selection,
                        format_capabilities: formats,
                        scale: target.scale,
                        refresh_millihz: target.refresh_millihz,
                        transform: target.transform,
                        mapping: target.mapping,
                        vrr: sophia_protocol::OutputVrrPolicy::Disabled,
                        pending_callback: None,
                        completion_mode: LiveProductionKmsCompletionMode::PageFlipPreferred,
                        completion_fence_status:
                            crate::LibdrmNativeCompletionFenceStatus::Unsupported,
                        out_fence_retirements: 0,
                        late_page_flip_events: 0,
                        completion_fence_errors: 0,
                        output: sophia_engine::HeadlessOutput {
                            id: output_id,
                            size,
                            scale: 1,
                        },
                        target_generation: 1,
                        submitted_at: None,
                        submitted_ust_usec: None,
                        pending_nonzero_pixel_bytes: 0,
                        last_checksum: 0,
                        submitted_checksum: None,
                        submitted_sequence: None,
                        pending_content: None,
                        rendering_content: None,
                        submitted_content: None,
                        submitted_direct: false,
                        layout_witness: layout_retirement::NativeLayoutWitnessState::default(),
                        presented_direct: false,
                        presented_content: None,
                        presented_logical_checksum: 0,
                        presented_submissions: 0,
                        service_skew_baseline: None,
                        presented_submission_ust_usec: 0,
                        presented_page_flip_ust_usec: 0,
                        presented_submit_to_page_flip: Duration::ZERO,
                        submissions: 0,
                        retirements: 0,
                        callback_accepted: 0,
                        initial_modeset_submission: None,
                        nonzero_exports: 0,
                        last_submit_report: None,
                        pending_cursor: None,
                        pending_cursor_since: None,
                        committed_cursor: None,
                        cursor_properties: None,
                        prepared_cursor_ride: None,
                        displayed_scanout: None,
                        displayed_group_frame: None,
                        scanout_submission: None,
                        prepared_scanout: None,
                        prepared_group_frame: None,
                        prepared_worker_was_in_flight: false,
                        scanout_cleanup: None,
                        scanout_cleanup_group_frame: None,
                        scanout_in_flight_ticks: 0,
                        last_callback_serial: None,
                        submitted_group_frame: None,
                        output_frames: OutputFramePresentationState::new(
                            sophia_engine::HeadlessOutput {
                                id: output_id,
                                size,
                                scale: 1,
                            },
                        )
                        .map_err(|error| {
                            format!(
                                "native output has invalid compositor display-list state: {error}"
                            )
                        })?,
                    });
                }
                let callback_capacity = session.heads().len().max(1);
                groups.push(LiveProductionNativeGroup {
                    session,
                    callbacks: Vec::with_capacity(callback_capacity),
                    timestamps: Vec::with_capacity(callback_capacity),
                    renderer_core: None,
                });
            }
            // A head and its exporter are one physical scanout slot. Keep them
            // together while ordering logical outputs; sorting only `heads`
            // silently retargets exporters whenever discovery order differs from
            // logical-output order.
            let mut head_exporters = heads.into_iter().zip(exporters).collect::<Vec<_>>();
            head_exporters.sort_by_key(|(head, _)| {
                (
                    head.output.id,
                    presentation_outputs.primary_head(head.output.id) != Some(head.head),
                    head.selection.connector_id(),
                )
            });
            let mut sorted_heads = Vec::with_capacity(head_exporters.len());
            let mut sorted_exporters = Vec::with_capacity(head_exporters.len());
            for (head, exporter) in head_exporters {
                sorted_heads.push(head);
                sorted_exporters.push(exporter);
            }
            let heads = sorted_heads;
            let exporters = sorted_exporters;
            let mut logical_outputs = Vec::new();
            for head in &heads {
                if logical_outputs
                    .iter()
                    .any(|output: &sophia_engine::HeadlessOutput| output.id == head.output.id)
                {
                    continue;
                }
                logical_outputs.push(head.output);
            }
            let mut output_lifecycles = BTreeMap::new();
            for output in heads
                .iter()
                .map(|head| head.output.id)
                .collect::<BTreeSet<_>>()
            {
                let members = heads
                    .iter()
                    .filter(|head| head.output.id == output)
                    .map(|head| head.head);
                let lifecycle = LiveProductionMirrorGroupLifecycle::new(output, members)
                    .expect("a native logical output has at least one physical head");
                output_lifecycles.insert(output, lifecycle);
            }
            Ok(Self {
                groups,
                heads,
                logical_outputs,
                discovered_outputs: connector_records.len(),
                presentation_outputs: presentation_output_count,
                submissions: 0,
                submit_deferred: 0,
                submit_failures: 0,
                retirements: 0,
                retire_failures: 0,
                max_in_flight_ticks: 0,
                max_in_flight_per_output: 0,
                max_service_skew: 0,
                direct_scanout_admissible: false,
                translation_motion_active: false,
                direct_scanout_admitted: false,
                pending_frame_supersessions: 0,
                cost: crate::DirectScanoutCost::default(),
                max_submit_to_page_flip: Duration::ZERO,
                callback_accepted: 0,
                callback_rejected: 0,
                callback_queue_saturated: 0,
                nonzero_exports: 0,
                exporters,
                image_import_devices: Vec::new(),
                render_devices: render_devices::LiveRenderDeviceState::new(),
                output_lifecycles,
                output_cohorts: BTreeMap::new(),
                deferred_mirror_generations: BTreeMap::new(),
                output_topology_preparation: None,
                output_topology_cleanup: Vec::new(),
                head_table,
                next_frame_id: 1,
                next_head_candidate_id: 1,
                production_page_flips,
                kernel_page_flip_timestamps: 0,
                kernel_page_flip_timestamp_missing: 0,
                kernel_page_flip_ust: BTreeMap::new(),
                vsync_overlap_rejections: 0,
                page_flip_phase_rejections: 0,
                cursor_updates: 0,
                cursor_hidden_updates: 0,
                cursor_updates_queued: 0,
                cursor_updates_coalesced: 0,
                cursor_updates_ridden: 0,
                cursor_only_commits: 0,
                cursor_combined_drops: 0,
                cursor_legacy_fallbacks: 0,
                cursor_path: crate::HardwareCursorPath::LegacyIoctl,
                cursor_initialization_deferrals: 0,
                cursor_updates_primary_in_flight: 0,
                cursor_update_failures: 0,
                max_cursor_initialization: Duration::ZERO,
                max_cursor_update: Duration::ZERO,
                max_cursor_queue_delay: Duration::ZERO,
            })
        }

        pub fn clone_render_device_file(&self) -> std::io::Result<std::fs::File> {
            self.groups
                .first()
                .ok_or_else(|| std::io::Error::other("native scanout has no DRM device group"))?
                .session
                .card()
                .try_clone_file()
        }

        /// Queries the fixed device inventory before frontend construction.
        /// Connector changes within these groups cannot broaden this intersection.
        pub fn dma_buf_import_formats(
            &self,
        ) -> Result<Vec<LiveDmaBufImportFormat>, LiveDmaBufCapabilityError> {
            if self.groups.is_empty() {
                return Err(LiveDmaBufCapabilityError::DeviceUnavailable);
            }
            let devices = self
                .groups
                .iter()
                .map(|group| {
                    let device = group
                        .session
                        .card()
                        .try_clone_file()
                        .map_err(|_| LiveDmaBufCapabilityError::DeviceUnavailable)?;
                    query_dma_buf_import_formats(device)
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(common_dma_buf_import_formats(&devices))
        }

        /// The desktop's logical outputs, one per `OutputId`.
        ///
        /// Heads are per connector and a mirror group has several sharing one
        /// logical output, so returning one entry per head would present a group as
        /// two outputs side by side. Everything above this is a topology, and a
        /// topology counts screens rather than cables.
        pub fn outputs(&self) -> Vec<sophia_engine::HeadlessOutput> {
            self.logical_outputs.clone()
        }

        /// The configured primary head driving a logical output.
        ///
        /// Named for what it returns. It was `output_index`, which read like a
        /// position in the output list and was passed one by four callers -- a
        /// coincidence that holds only while every output has exactly one head.
        ///
        /// Correct only for logical-output authority: reading the primary card,
        /// connector, or CRTC. A caller that submits, retires, or releases per
        /// head must use `head_indices` instead, or it will silently ignore the
        /// rest of a mirror group.
        pub fn primary_head_index(&self, output: OutputId) -> Option<usize> {
            if let Some(primary) = self
                .output_lifecycles
                .get(&output)
                .map(LiveProductionMirrorGroupLifecycle::primary_head)
                && let Some(index) = self.heads.iter().position(|head| {
                    head.enabled && head.output.id == output && head.head == primary
                })
            {
                return Some(index);
            }
            self.heads
                .iter()
                .position(|head| head.enabled && head.output.id == output)
        }

        /// The head driving a named connector.
        ///
        /// The one lookup that is exact for a mirror group: every head has its own
        /// connector even when several share a logical output, so a caller that must
        /// address one specific head asks by connector rather than by output.
        pub fn head_index_for_output_head(
            &self,
            output: OutputId,
            head_id: sophia_engine::RenderHeadId,
        ) -> Option<usize> {
            self.heads
                .iter()
                .position(|head| head.enabled && head.output.id == output && head.head == head_id)
        }

        /// Resolves a connector id when the caller has already established that
        /// the capability namespace is unambiguous.
        ///
        /// DRM connector ids are card-local, so callback and presentation paths
        /// must use the output-qualified lookup above. Startup topology mapping
        /// retains this facade because its named capability set is validated
        /// before it reaches this point.
        pub fn head_index_for_head(&self, head_id: sophia_engine::RenderHeadId) -> Option<usize> {
            self.heads.iter().position(|head| head.head == head_id)
        }

        /// Resolves a native connector id through the head table. This is the
        /// backend-boundary translation for callers that hold configuration or
        /// capability facts (connector names and ids) rather than heads.
        pub fn head_index_for_native_connector(&self, connector_id: u32) -> Option<usize> {
            let record = self
                .head_table
                .records()
                .iter()
                .find(|record| record.connector_id == connector_id)?;
            self.head_index_for_head(record.head)
        }

        /// Every head driving a logical output, in head order.
        pub fn head_indices(&self, output: OutputId) -> Vec<usize> {
            self.heads
                .iter()
                .enumerate()
                .filter(|(_, head)| head.enabled && head.output.id == output)
                .map(|(index, _)| index)
                .collect()
        }

        /// How many connectors drive each logical output, in output order.
        ///
        /// The topology owner compares this beside the output list: losing one head
        /// of a mirror group leaves the logical outputs unchanged, so a comparison
        /// on outputs alone would call that no change at all.
        pub fn head_fingerprint(&self) -> Vec<(OutputId, usize)> {
            let mut counts: BTreeMap<OutputId, usize> = BTreeMap::new();
            for head in self.heads.iter().filter(|head| head.enabled) {
                *counts.entry(head.output.id).or_default() += 1;
            }
            counts.into_iter().collect()
        }

        fn allocate_frame_id(&mut self) -> LiveProductionNativeFrameId {
            let frame = LiveProductionNativeFrameId::from_raw(self.next_frame_id);
            self.next_frame_id = self
                .next_frame_id
                .checked_add(1)
                .expect("native frame ID space exhausted");
            frame
        }

        fn allocate_head_candidate_id(&mut self) -> sophia_engine::HeadFrameCandidateId {
            let candidate =
                sophia_engine::HeadFrameCandidateId::from_raw(self.next_head_candidate_id);
            self.next_head_candidate_id = self
                .next_head_candidate_id
                .checked_add(1)
                .expect("native head candidate ID space exhausted");
            candidate
        }

        /// The stalled head's index and how long its flip has been outstanding.
        ///
        /// The index rather than the output alone, because attributing a stall
        /// needs the head's own history and the output cannot reach it: a mirror
        /// group's heads share an output.
        pub fn page_flip_hard_stall(&self) -> Option<(usize, Duration)> {
            self.heads.iter().enumerate().find_map(|(index, head)| {
                let age = head.submitted_at.map(|submitted| submitted.elapsed());
                (reduce_live_production_page_flip_watchdog(
                    age,
                    LIVE_PRODUCTION_PAGE_FLIP_HARD_STALL,
                ) == LiveProductionPageFlipWatchdogStatus::HardStall)
                    .then_some((index, age.unwrap_or_default()))
            })
        }

        pub fn ensure_page_flip_progress(&self) -> Result<(), Box<dyn std::error::Error>> {
            let Some((index, age)) = self.page_flip_hard_stall() else {
                return Ok(());
            };
            let head = &self.heads[index];
            // Its own record rather than another `sophia_live_native_page_flip`
            // status: that name's schema=1 is pinned by matchers reading retained
            // evidence, and a stall's fields have nothing in common with a flip's
            // lifecycle transitions. Widening the shared name would have meant
            // bumping every status under it and invalidating runs already on disk.
            //
            // Two of these terminated a session before any input and neither
            // could be attributed, because `output` and `age` do not distinguish
            // a head that never received its first vblank from one that stopped
            // receiving them, nor a stalled head from a stalled group. The
            // fields below are the ones that separate those cases, and they are
            // already on the head at the moment the stall is declared.
            //
            // The other heads' ages come along because a stall shared across a
            // mirror group is a different fault from a stall on one connector,
            // and the first head to cross the boundary is not necessarily the
            // one that caused it.
            let peers = self
                .heads
                .iter()
                .enumerate()
                .filter(|(peer, _)| *peer != index)
                .map(|(peer, head)| {
                    format!(
                        "{peer}:{}",
                        head.submitted_at.map_or(-1i64, |submitted| i64::try_from(
                            submitted.elapsed().as_millis()
                        )
                        .unwrap_or(i64::MAX))
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            // Instantaneous state explains what can retire now; cumulative
            // counters explain whether this process ever decoded, rejected, or
            // emitted evidence. An empty final read is not an attribution.
            let diagnostics = self.groups[head.group]
                .session
                .page_flip_poller_diagnostics();
            let cumulative = self.groups[head.group]
                .session
                .page_flip_poller_cumulative_diagnostics();
            tracing::error!(
                "sophia_live_native_page_flip_stall schema=3 status=hard_stall output={} head={} index={} group={} age_ms={} generation={} submissions={} retirements={} callbacks={} ever_retired={} callback_serial={} in_flight_ticks={} submitted_sequence={} peer_age_ms=[{}] completion_mode={:?} completion_fence={:?} completion_ledger_pending={} out_fence_retirements={} late_page_flip_events={} completion_fence_errors={} poller_pending={} poller_routes={} poller_last_read={:?} poller_last_decoded={} poller_last_rejected={} poller_read_calls={} poller_would_block_reads={} poller_read_failures={} poller_decoded_total={} poller_rejected_total={} poller_emitted_total={} action=terminate_session",
                head.output.id.raw(),
                head.head.raw(),
                index,
                head.group,
                age.as_millis(),
                head.target_generation,
                head.submissions,
                head.retirements,
                head.callback_accepted,
                head.retirements != 0,
                head.last_callback_serial
                    .map_or_else(|| "none".to_owned(), |serial| serial.to_string()),
                head.scanout_in_flight_ticks,
                head.submitted_sequence
                    .map_or_else(|| "none".to_owned(), |sequence| sequence.to_string()),
                peers,
                head.completion_mode,
                head.completion_fence_status,
                head.pending_callback.is_some(),
                head.out_fence_retirements,
                head.late_page_flip_events,
                head.completion_fence_errors,
                diagnostics.pending_callbacks,
                diagnostics.route_count,
                diagnostics.last_read_loop.status,
                diagnostics.last_read_loop.decoded_callbacks,
                diagnostics.last_read_loop.rejected_callbacks,
                cumulative.read_calls,
                cumulative.would_block_reads,
                cumulative.read_failures,
                cumulative.decoded_callbacks,
                cumulative.rejected_callbacks,
                cumulative.emitted_callbacks,
            );
            Err(format!(
                "native page flip exceeded the {} ms hard-stall boundary on head {} after {} retirements",
                LIVE_PRODUCTION_PAGE_FLIP_HARD_STALL.as_millis(),
                head.head.raw(),
                head.retirements,
            )
            .into())
        }

        pub fn selection(&self, index: usize) -> crate::LibdrmNativePrimaryPlaneSelection {
            self.heads[index].selection
        }

        pub fn card(&self, index: usize) -> &crate::RealAtomicScanoutCard {
            self.groups[self.heads[index].group].session.card()
        }

        /// One head and its output's exporter together.
        ///
        /// They live in different tables now, and most work touches both. Handing
        /// out the pair from one place keeps every caller from having to spell out
        /// the disjoint borrow itself.
        fn head_and_exporter(
            &mut self,
            index: usize,
            output: OutputId,
        ) -> (
            &mut LiveProductionNativeHead,
            &mut crate::NativeGbmRenderedScanoutBufferDiscoveryExporter<
                crate::RealAtomicScanoutRenderDeviceDiscovery,
            >,
        ) {
            let _ = output;
            (
                &mut self.heads[index],
                self.exporters
                    .get_mut(index)
                    .expect("a registered head has an exporter"),
            )
        }

        /// The exporter backing an output's configured primary head, for reads.
        ///
        /// A caller that composes or exports per head must resolve the head
        /// first, or a group's other connectors get nothing.
        fn exporter(
            &self,
            output: OutputId,
        ) -> Option<
            &crate::NativeGbmRenderedScanoutBufferDiscoveryExporter<
                crate::RealAtomicScanoutRenderDeviceDiscovery,
            >,
        > {
            self.exporters.get(self.primary_head_index(output)?)
        }

        /// The exporter backing an output's configured primary head.
        fn exporter_mut(
            &mut self,
            output: OutputId,
        ) -> Result<
            &mut crate::NativeGbmRenderedScanoutBufferDiscoveryExporter<
                crate::RealAtomicScanoutRenderDeviceDiscovery,
            >,
            Box<dyn std::error::Error>,
        > {
            let index = self.primary_head(output)?;
            self.exporters
                .get_mut(index)
                .ok_or_else(|| format!("native output {} has no exporter", output.raw()).into())
        }

        /// The head this logical output is addressed through.
        ///
        /// Every per-head entry point below resolves through this rather than
        /// taking a position, because the caller's position is an index into
        /// *outputs* and the two stop agreeing the moment a mirror group exists.
        fn primary_head(&self, output: OutputId) -> Result<usize, Box<dyn std::error::Error>> {
            self.primary_head_index(output)
                .ok_or_else(|| format!("native output {} has no head", output.raw()).into())
        }

        /// The card driving a logical output.
        pub fn card_for_output(&self, output: OutputId) -> Option<&crate::RealAtomicScanoutCard> {
            self.primary_head_index(output)
                .map(|index| self.card(index))
        }

        /// The primary connector selection of a logical output.
        pub fn selection_for_output(
            &self,
            output: OutputId,
        ) -> Option<crate::LibdrmNativePrimaryPlaneSelection> {
            self.primary_head_index(output)
                .map(|index| self.heads[index].selection)
        }

        pub fn run_tick(
            &mut self,
            output: OutputId,
            runtime: &mut crate::LiveBackendRuntimeAssembly,
            input: CompositorBackendTickInput,
        ) -> Result<crate::LiveBackendRuntimeTickReport, Box<dyn std::error::Error>> {
            self.retry_output_topology_cleanup();
            if !self.output_topology_allows_frame_service() {
                return Err(
                    "ordinary native frame scheduling is quarantined during topology preparation"
                        .into(),
                );
            }
            if self.head_indices(output).len() > 1 {
                let report = self.run_mirror_group_scene_tick(output, runtime, input)?;
                if self.mirror_poison_drained(output) {
                    return Err("mirror generation failed after physical ownership drained".into());
                }
                // Sampled here too: a mirror output is where concurrent depth
                // is expected to exceed one, so omitting it would leave the
                // measurement blind to the case it exists to describe.
                self.observe_in_flight_depth();
                return Ok(report);
            }
            let index = self.primary_head(output)?;
            if !self.exporter_mut(output)?.pending_frame() {
                self.retire_ready_and_retry_cleanup(output, runtime)?;
                return Ok(runtime.run_tick(input)?);
            }
            self.prepare_layout_probe_turn(index);
            let group = self.heads[index].group;
            // Arm the cursor to ride this frame's commit, when one is
            // pending. The request is being built anyway, so the ride costs
            // nothing -- and it is the only way a cursor moves while frames
            // are flowing, since the CRTC is then never free for a
            // cursor-only commit. Settled below only when the submit was
            // accepted; a deferred or failed submission leaves the position
            // pending, because a cursor must never be lost to a frame that
            // did not happen.
            let cursor_ride = self.arm_cursor_ride(index);
            runtime.set_cursor_ride_request(output, cursor_ride.map(|(cursor, _)| cursor));
            let (report, exported_nonzero, worker_was_in_flight, submitted_direct) = {
                let groups = &mut self.groups;
                let head = &mut self.heads[index];
                let exporter = self
                    .exporters
                    .get_mut(index)
                    .ok_or_else(|| format!("native output {} has no exporter", output.raw()))?;
                let worker_was_in_flight = exporter.worker_in_flight();
                let export_attempts_before = exporter.cpu_frame_export_attempts();
                let direct_flips_before = exporter.direct_scanout_flips();
                let report = runtime
                    .run_tick_with_native_gbm_rendered_primary_plane_scanout_exporter_with(
                        input,
                        groups[group].session.card(),
                        exporter,
                    )?;
                let exported_nonzero = exporter.cpu_frame_export_attempts()
                    > export_attempts_before
                    && head.pending_nonzero_pixel_bytes > 0;
                // Whether the submission this tick produced -- if it produced
                // one -- put the client's own buffer on the plane. Read as a
                // difference rather than a flag because the exporter is
                // several calls away from the submit that consumed its export,
                // and a flag would have to be cleared by whichever of those
                // calls ran last.
                let submitted_direct = exporter.direct_scanout_flips() > direct_flips_before;
                if !exporter.pending_cpu_frame() {
                    head.pending_nonzero_pixel_bytes = 0;
                }
                (
                    report,
                    exported_nonzero,
                    worker_was_in_flight,
                    submitted_direct,
                )
            };
            if exported_nonzero {
                self.nonzero_exports = self.nonzero_exports.saturating_add(1);
                self.heads[index].nonzero_exports =
                    self.heads[index].nonzero_exports.saturating_add(1);
            }
            if let Some(retire) = report.rendered_primary_plane_scanout_retire {
                self.observe_retire(index, retire);
            }
            self.observe_callbacks(index, report.page_flip_callbacks.clone());
            let worker_is_in_flight = self.exporter(output).is_some_and(
                crate::NativeGbmRenderedScanoutBufferDiscoveryExporter::worker_in_flight,
            );
            let renderer_started = {
                let head = &mut self.heads[index];
                advance_live_production_renderer_content(
                    worker_was_in_flight,
                    worker_is_in_flight,
                    &mut head.pending_content,
                    &mut head.rendering_content,
                )?
            };
            if renderer_started && self.heads[index].output_frames.pending().is_some() {
                self.heads[index]
                    .output_frames
                    .mark_rendering()
                    .map_err(|error| {
                        format!("compositor display-list render transition failed: {error}")
                    })?;
            }
            if let Some(submit) = report.rendered_primary_plane_scanout_submit {
                self.heads[index].last_submit_report = Some(submit);
                use crate::LiveTrackedRenderedPrimaryPlaneScanoutSubmitStatus as Status;
                match submit.status {
                    Status::SubmittedWaitingForPageFlip => {
                        let pending_before = self.heads[index].pending_content;
                        let rendering_before = self.heads[index].rendering_content;
                        let content = if worker_was_in_flight {
                            self.heads[index].rendering_content.take()
                        } else {
                            self.heads[index].pending_content.take()
                        };
                        if content.is_none() {
                            return Err(format!(
                                "accepted native submission lost its content identity: \
output={} selected_slot={} worker_was_in_flight={} worker_is_in_flight={} \
pending_before={pending_before:?} rendering_before={rendering_before:?} exporter_pending={}",
                                output.raw(),
                                if worker_was_in_flight {
                                    "rendering"
                                } else {
                                    "pending"
                                },
                                worker_was_in_flight,
                                worker_is_in_flight,
                                self.exporter(output)
                                    .is_some_and(|exporter| exporter.pending_frame()),
                            )
                            .into());
                        }
                        // This is the head's pixel proof, not a measurement of
                        // this frame: a readback costs a whole framebuffer, so
                        // a renderer context takes a bounded number of them and
                        // then keeps the last. It answers "this head has put
                        // light on a screen", which is what startup readiness
                        // asks of it.
                        let content = content.map(|content| {
                            content.with_nonzero_rgb_pixels(
                                self.exporter(output).map_or(0, |exporter| {
                                    exporter.composition_nonzero_rgb_pixels()
                                }),
                            )
                        });
                        if worker_was_in_flight
                            && self.heads[index].output_frames.rendering().is_some()
                        {
                            self.heads[index]
                                .output_frames
                                .promote_rendering_to_submitted()
                                .map_err(|error| {
                                    format!(
                                        "compositor display-list worker promotion failed: {error}"
                                    )
                                })?;
                        } else if !worker_was_in_flight
                            && self.heads[index].output_frames.pending().is_some()
                        {
                            self.heads[index]
                                .output_frames
                                .mark_submitted()
                                .map_err(|error| {
                                    format!(
                                        "compositor display-list submit transition failed: {error}"
                                    )
                                })?;
                        }
                        trace_live_native_lifecycle("kms_submit_accepted");
                        self.submissions = self.submissions.saturating_add(1);
                        self.heads[index].submissions =
                            self.heads[index].submissions.saturating_add(1);
                        self.heads[index].submitted_at = Some(Instant::now());
                        self.heads[index].submitted_ust_usec = Some(Self::monotonic_ust_usec());
                        self.heads[index].submitted_checksum =
                            Some(self.heads[index].last_checksum);
                        self.heads[index].submitted_sequence = Some(self.heads[index].submissions);
                        self.heads[index].submitted_content = content;
                        self.heads[index].submitted_direct = submitted_direct;
                        self.observe_layout_witness_submit(index, submit);
                        // Settled only if the cursor actually rode: a
                        // combined commit the driver refused retries with the
                        // primary alone, and settling then would record a
                        // cursor the plane is not showing. The position stays
                        // pending instead, for a later commit.
                        if let Some((_, placement)) = cursor_ride {
                            if submit.cursor_dropped {
                                self.cursor_combined_drops =
                                    self.cursor_combined_drops.saturating_add(1);
                            } else {
                                self.settle_atomic_cursor(index, placement, true);
                            }
                        }
                        if matches!(
                            content,
                            Some(
                                LiveProductionScanoutContent::MixedPresent {
                                    nonzero_rgb_pixels: 1..,
                                    ..
                                } | LiveProductionScanoutContent::RetainedMixed {
                                    nonzero_rgb_pixels: 1..,
                                    ..
                                } | LiveProductionScanoutContent::HeadComposition {
                                    nonzero_rgb_pixels: 1..,
                                    ..
                                }
                            )
                        ) {
                            self.nonzero_exports = self.nonzero_exports.saturating_add(1);
                            self.heads[index].nonzero_exports =
                                self.heads[index].nonzero_exports.saturating_add(1);
                        }
                        let output = self.heads[index].output.id;
                        let cycle =
                            u64::try_from(self.heads[index].submissions).unwrap_or(u64::MAX);
                        let frame = content.map_or(0, |content| content.frame().raw());
                        tracing::trace!(
                            "sophia_live_native_page_flip schema=1 status=submitted output={} submission={} content={:?} frame={}",
                            output.raw(),
                            cycle,
                            content,
                            frame,
                        );
                        tracing::trace!(
                            "sophia_live_native_head_page_flip schema=2 status=submitted output={} head={} submission={} content={:?} frame={}",
                            output.raw(),
                            self.heads[index].head.raw(),
                            cycle,
                            content,
                            frame,
                        );
                        if let Err(error) = self.production_page_flips.submit(output, cycle) {
                            self.vsync_overlap_rejections =
                                self.vsync_overlap_rejections.saturating_add(1);
                            tracing::error!(
                                "sophia_live_native_pacing schema=1 status=submit_rejected output={} submission={} error={error:?}",
                                output.raw(),
                                cycle,
                            );
                        }
                    }
                    Status::ScanoutExportPending => {
                        self.submit_deferred = self.submit_deferred.saturating_add(1);
                    }
                    Status::AlreadyInFlight | Status::CleanupPending => {
                        self.submit_deferred = self.submit_deferred.saturating_add(1);
                    }
                    status => {
                        let failed_content = if worker_was_in_flight {
                            self.heads[index].rendering_content.take()
                        } else {
                            self.heads[index].pending_content.take()
                        };
                        if worker_was_in_flight {
                            self.heads[index].output_frames.discard_rendering();
                        } else {
                            self.heads[index].output_frames.discard_pending();
                        }
                        self.submit_failures = self.submit_failures.saturating_add(1);
                        tracing::warn!(
                            "sophia_live_native_submit schema=1 status=failed output={} reason={status:?} content={failed_content:?} export={:?} scanout_buffer={:?} resources={:?} framebuffer={:?} submit={:?} commit={:?}",
                            self.heads[index].output.id.raw(),
                            submit.export,
                            submit.scanout_buffer,
                            submit.resources,
                            submit.framebuffer,
                            submit.submit,
                            submit.commit_submit,
                        );
                    }
                }
            }
            self.max_in_flight_ticks = self
                .max_in_flight_ticks
                .max(report.rendered_primary_plane_scanout_in_flight_ticks);
            self.observe_in_flight_depth();
            Ok(report)
        }

        /// Record how many submissions this output holds right now.
        ///
        /// Sampled per tick rather than incremented at submit: the property is
        /// concurrent depth, and only a reading taken while submissions are
        /// live can observe it.
        fn observe_in_flight_depth(&mut self) {
            let depth = self.head_scanout_in_flight_count();
            self.max_in_flight_per_output = self.max_in_flight_per_output.max(depth);
            let supersessions: usize = self
                .exporters
                .iter()
                .map(|exporter| exporter.pending_frame_supersessions())
                .sum();
            self.pending_frame_supersessions = self.pending_frame_supersessions.max(supersessions);
            self.observe_service_skew();
        }

        /// How far one output's wait ran behind its siblings' service.
        ///
        /// While a head has a request outstanding, every render another head
        /// completes is that head being passed over. Taking the shared queue
        /// in order bounds this at one per sibling, which is the property the
        /// model states and the reason no scheduler is needed; measuring it
        /// is what turns that from an argument into evidence.
        ///
        /// Sampled on the tick rather than hooked at submit and completion,
        /// because the worker cannot see its own queue: a render already
        /// dequeued gives no way to know who was waiting behind it. Two
        /// completions inside one tick therefore read as one, so this is a
        /// lower bound on true skew -- it can miss a peak, never invent one.
        /// The structural guarantee remains FIFO service; this is the check
        /// that the implementation kept it.
        fn observe_service_skew(&mut self) {
            for index in 0..self.exporters.len() {
                if !self.heads[index].enabled {
                    continue;
                }
                if !self.exporters[index].worker_in_flight() {
                    self.heads[index].service_skew_baseline = None;
                    continue;
                }
                let siblings: usize = self
                    .exporters
                    .iter()
                    .enumerate()
                    .filter(|(other, _)| *other != index)
                    .filter_map(|(_, exporter)| exporter.worker_metrics())
                    .map(|metrics| metrics.completions)
                    .sum();
                match self.heads[index].service_skew_baseline {
                    None => self.heads[index].service_skew_baseline = Some(siblings),
                    Some(baseline) => {
                        self.max_service_skew =
                            self.max_service_skew.max(siblings.saturating_sub(baseline));
                    }
                }
            }
        }

        fn synthesize_out_fence_callback(&mut self, index: usize) -> crate::LivePageFlipCallback {
            let serial = self.heads[index]
                .last_callback_serial
                .unwrap_or_default()
                .saturating_add(1);
            self.heads[index].completion_mode =
                LiveProductionKmsCompletionMode::OutFenceAuthoritative;
            self.heads[index].out_fence_retirements =
                self.heads[index].out_fence_retirements.saturating_add(1);
            crate::LivePageFlipCallback {
                output: self.heads[index].output.id,
                head: self.heads[index].head,
                frame_serial: serial,
            }
        }

        fn service_mirror_group_retirement(
            &mut self,
            output: OutputId,
            runtime: &mut crate::LiveBackendRuntimeAssembly,
        ) -> LiveProductionMirrorRetirementReport {
            let indices = self.head_indices(output);
            let mut errors = Vec::new();
            // The card pump has already routed at most one completion into
            // each head's ledger. Admit those physical callbacks without
            // publishing a per-head logical flip; the group join below is the
            // only publisher of the output-level event.
            let mut page_flip_callbacks =
                crate::LivePageFlipCallbackQueueReport::with_accepted_capacity(indices.len());
            let mut completion_sources = Vec::with_capacity(indices.len());
            for head_index in indices.iter().copied() {
                let completion = self.heads[head_index]
                    .pending_callback
                    .take()
                    .map(|callback| (callback, LiveProductionKmsCompletionSource::PageFlipEvent));
                let completion = if completion.is_some() {
                    completion
                } else {
                    match self.heads[head_index]
                        .scanout_submission
                        .as_ref()
                        .map(crate::LiveRenderedPrimaryPlaneScanoutSubmission::completion_fence_status)
                        .transpose()
                    {
                        Ok(status) => {
                            let status = status
                                .unwrap_or(crate::LibdrmNativeCompletionFenceStatus::Unsupported);
                            self.heads[head_index].completion_fence_status = status;
                            if status == crate::LibdrmNativeCompletionFenceStatus::Signaled {
                                Some((
                                    self.synthesize_out_fence_callback(head_index),
                                    LiveProductionKmsCompletionSource::OutFence,
                                ))
                            } else {
                                None
                            }
                        }
                        Err(error) => {
                            self.heads[head_index].completion_fence_errors = self.heads[head_index]
                                .completion_fence_errors
                                .saturating_add(1);
                            errors.push(format!(
                                "mirror head {} completion fence poll failed: {error}",
                                self.heads[head_index].head.raw(),
                            ));
                            None
                        }
                    }
                };
                let Some((callback, source)) = completion else {
                    continue;
                };
                let observation = runtime.observe_mirror_page_flip_callback(callback);
                if observation.decision == crate::LivePageFlipCallbackDecision::Accepted {
                    completion_sources.push(source);
                }
                page_flip_callbacks.record_observation(callback, observation);
            }
            self.callback_rejected = self.callback_rejected.saturating_add(
                page_flip_callbacks.rejected_unexpected_output
                    + page_flip_callbacks.rejected_stale_frame_serial,
            );

            let mut completed_retire = None;
            let mut completed_serial = None;
            for (callback, completion_source) in page_flip_callbacks
                .accepted_callbacks
                .iter()
                .copied()
                .zip(completion_sources)
            {
                let Some(head_index) = self.head_index_for_output_head(output, callback.head)
                else {
                    self.callback_rejected = self.callback_rejected.saturating_add(1);
                    errors.push(format!(
                        "mirror callback referenced unknown head {}",
                        callback.head.raw()
                    ));
                    continue;
                };
                let Some(mut submission) = self.heads[head_index].scanout_submission.take() else {
                    errors.push(format!(
                        "mirror head {} callback has no physical submission",
                        callback.head.raw()
                    ));
                    continue;
                };
                if self.heads[head_index]
                    .last_callback_serial
                    .is_some_and(|serial| callback.frame_serial <= serial)
                {
                    self.heads[head_index].scanout_submission = Some(submission);
                    self.callback_rejected = self.callback_rejected.saturating_add(1);
                    continue;
                }
                self.heads[head_index].last_callback_serial = Some(callback.frame_serial);
                let callback_ust = self.completion_ust_usec(
                    output,
                    callback.head,
                    callback.frame_serial,
                    completion_source,
                );
                let submitted_ust_usec = self.heads[head_index].submitted_ust_usec.take();
                let submit_to_page_flip = submitted_ust_usec
                    .and_then(|submitted| callback_ust.checked_sub(submitted))
                    .map(Duration::from_micros)
                    .or_else(|| {
                        self.heads[head_index]
                            .submitted_at
                            .map(|submitted| submitted.elapsed())
                    })
                    .unwrap_or_default();
                self.max_submit_to_page_flip =
                    self.max_submit_to_page_flip.max(submit_to_page_flip);
                self.heads[head_index].presented_submission_ust_usec =
                    submitted_ust_usec.unwrap_or_default();
                self.heads[head_index].presented_page_flip_ust_usec = callback_ust;
                self.heads[head_index].presented_submit_to_page_flip = submit_to_page_flip;
                // A mirror head composes by construction -- eligibility
                // requires a single-head plan shape -- so this is recorded
                // as composed rather than asked.
                self.cost.record_submit_to_flip(false, submit_to_page_flip);
                let Some(frame) = self.heads[head_index].submitted_group_frame.take() else {
                    errors.push(format!(
                        "mirror head {} callback has no logical generation",
                        callback.head.raw()
                    ));
                    continue;
                };
                submission.clear_completion_fence();
                let previous_frame = self.heads[head_index].displayed_group_frame.replace(frame);
                if let Some(previous) = self.heads[head_index].displayed_scanout.replace(submission)
                {
                    let crate::LiveRenderedPrimaryPlaneScanoutSubmission {
                        scanout_buffer,
                        primary_plane,
                        ..
                    } = previous;
                    let retired = primary_plane.retire(self.card(head_index));
                    if let Some(primary_plane) = retired.cleanup {
                        self.heads[head_index].scanout_cleanup =
                            Some(crate::LiveRenderedPrimaryPlaneScanoutCleanup {
                                scanout_buffer,
                                primary_plane,
                            });
                        self.heads[head_index].scanout_cleanup_group_frame = previous_frame;
                    } else if let Some(previous_frame) = previous_frame
                        && let Some(cohort) = self.output_cohorts.get_mut(&(output, previous_frame))
                    {
                        let _ = cohort.mark_cleanup_complete(callback.head);
                    }
                }
                self.heads[head_index].submitted_at = None;
                self.heads[head_index].scanout_in_flight_ticks = 0;
                self.heads[head_index].retirements =
                    self.heads[head_index].retirements.saturating_add(1);
                self.heads[head_index].callback_accepted =
                    self.heads[head_index].callback_accepted.saturating_add(1);
                self.retirements = self.retirements.saturating_add(1);
                self.callback_accepted = self.callback_accepted.saturating_add(1);
                self.heads[head_index].presented_content =
                    self.heads[head_index].submitted_content.take();
                self.heads[head_index].presented_logical_checksum = self.heads[head_index]
                    .submitted_checksum
                    .take()
                    .unwrap_or_default();
                if let Some(submission) = self.heads[head_index].submitted_sequence.take() {
                    self.heads[head_index].presented_submissions = submission;
                }
                if self.heads[head_index].output_frames.submitted().is_some() {
                    match self.heads[head_index].output_frames.mark_presented() {
                        Ok(presented) => {
                            trace_presented_output_damage(
                                "presented",
                                self.heads[head_index].output.id,
                                &presented,
                            );
                            trace_presented_mirror_head_damage(
                                output,
                                callback.head,
                                frame,
                                &presented,
                            );
                        }
                        Err(error) => errors.push(format!(
                            "mirror display-list presentation transition failed: {error}"
                        )),
                    }
                }
                if let Some(cohort) = self.output_cohorts.get_mut(&(output, frame)) {
                    let transition = cohort.mark_flipped(callback.head, callback_ust);
                    if !matches!(
                        transition,
                        sophia_engine::OutputPresentationTransition::Accepted
                            | sophia_engine::OutputPresentationTransition::PhaseReady
                    ) {
                        errors.push(format!(
                            "mirror head {} entered invalid cohort flip transition {transition:?}",
                            callback.head.raw(),
                        ));
                    }
                }
                tracing::info!(
                    "sophia_live_native_head_completion schema=1 status=accepted output={} head={} callbacks=1 completion_source={} completion_serial={} frame={}",
                    output.raw(),
                    callback.head.raw(),
                    completion_source.label(),
                    callback.frame_serial,
                    frame.raw(),
                );
                trace_native_head_retirement(
                    output.raw(),
                    callback.head.raw(),
                    self.heads[head_index].presented_submissions,
                    frame.raw(),
                );
                if self
                    .output_lifecycles
                    .get(&output)
                    .is_some_and(LiveProductionMirrorGroupLifecycle::failed)
                {
                    continue;
                }
                let lifecycle = self
                    .output_lifecycles
                    .get_mut(&output)
                    .expect("mirror output has a lifecycle");
                if !lifecycle.observe_flip_timing(
                    callback.head,
                    frame,
                    callback.frame_serial,
                    callback_ust,
                ) {
                    errors.push(format!(
                        "mirror head {} callback timing named the wrong generation",
                        callback.head.raw()
                    ));
                    continue;
                }
                let transition = lifecycle.mark_flipped(callback.head, frame);
                match transition {
                    LiveProductionMirrorHeadTransition::GroupReady => {
                        let Some((logical_serial, logical_ust)) = self
                            .output_lifecycles
                            .get(&output)
                            .and_then(LiveProductionMirrorGroupLifecycle::flip_timing)
                        else {
                            errors.push(
                                "completed mirror generation has no timing evidence".to_owned(),
                            );
                            continue;
                        };
                        let mut presented = true;
                        let native_content =
                            self.heads[head_index].presented_content.map(|content| {
                                LiveProductionNativeRetirementContent {
                                    content,
                                    submission: u64::try_from(
                                        self.heads[head_index].presented_submissions,
                                    )
                                    .unwrap_or(u64::MAX),
                                    direct: false,
                                    layout_witness: None,
                                }
                            });
                        if let Err(error) = self.production_page_flips.observe_native_page_flip(
                            output,
                            logical_serial,
                            logical_ust,
                            native_content,
                        ) {
                            self.page_flip_phase_rejections =
                                self.page_flip_phase_rejections.saturating_add(1);
                            errors.push(format!(
                                "mirror logical page-flip retirement was rejected: {error:?}"
                            ));
                            presented = false;
                        }
                        let presented_content = self.heads[head_index].presented_content;
                        let presented_logical_checksum =
                            self.heads[head_index].presented_logical_checksum;
                        if presented_content.is_none_or(|content| content.frame() != frame) {
                            errors.push(
                                "mirror primary presented the wrong content identity".to_owned(),
                            );
                            presented = false;
                        }
                        if let Some(content) = presented_content
                            && presented
                        {
                            tracing::info!(
                                "sophia_live_mirror_pacing schema=1 status=primary_presented output={} primary={} frame={}",
                                output.raw(),
                                callback.head.raw(),
                                frame.raw(),
                            );
                            tracing::info!(
                                "sophia_live_mirror_generation schema=2 status=presented output={} frame={} source={} logical_content_checksum={}",
                                output.raw(),
                                frame.raw(),
                                content.source_label(),
                                presented_logical_checksum,
                            );
                        }
                        if presented {
                            completed_retire = Some(crate::LiveTrackedRenderedPrimaryPlaneScanoutRetireReport {
                                status: crate::LiveTrackedRenderedPrimaryPlaneScanoutRetireStatus::RetiredAfterPageFlip,
                                layout_witness: None,
                                destroy: None,
                                runtime_scanout_state: Some(crate::RuntimeScanoutState::Retired),
                                in_flight: false,
                                in_flight_ticks: 0,
                                cleanup_pending: false,
                            });
                            completed_serial = Some(logical_serial);
                        }
                    }
                    LiveProductionMirrorHeadTransition::Accepted => {}
                    invalid => errors.push(format!(
                        "mirror-head {} entered invalid flipped transition {invalid:?}",
                        callback.head.raw(),
                    )),
                }
            }

            // Cleanup is ownership work, not scheduling. Always visit every
            // head, even when callback processing above found an error.
            for head_index in indices.iter().copied() {
                if let Some(cleanup) = self.heads[head_index].scanout_cleanup.take() {
                    let retried = crate::retry_rendered_primary_plane_scanout_cleanup(
                        self.card(head_index),
                        cleanup,
                    );
                    self.heads[head_index].scanout_cleanup = retried.cleanup;
                    if self.heads[head_index].scanout_cleanup.is_some() {
                        self.retire_failures = self.retire_failures.saturating_add(1);
                    } else if let Some(cleanup_frame) =
                        self.heads[head_index].scanout_cleanup_group_frame.take()
                        && let Some(cohort) = self.output_cohorts.get_mut(&(output, cleanup_frame))
                    {
                        let transition = cohort.mark_cleanup_complete(self.heads[head_index].head);
                        if !matches!(
                            transition,
                            sophia_engine::OutputPresentationTransition::Accepted
                                | sophia_engine::OutputPresentationTransition::PhaseReady
                        ) {
                            errors.push(format!(
                                "mirror head {} entered invalid retried cleanup transition {transition:?}",
                                self.heads[head_index].head.raw(),
                            ));
                        }
                    }
                }
            }

            self.output_cohorts
                .retain(|(cohort_output, frame), cohort| {
                    let releasable = *cohort_output == output && cohort.generation_releasable();
                    if releasable {
                        tracing::info!(
                            "sophia_live_mirror_pacing schema=1 status=released output={} frame={}",
                            output.raw(),
                            frame.raw(),
                        );
                    }
                    !releasable
                });

            LiveProductionMirrorRetirementReport {
                page_flip_callbacks,
                completed_retire,
                completed_serial,
                errors,
            }
        }

        fn publish_mirror_group_page_flip(
            &self,
            output: OutputId,
            runtime: &mut crate::LiveBackendRuntimeAssembly,
            completed_serial: Option<u64>,
        ) -> Option<crate::LivePageFlipEvent> {
            if completed_serial.is_none()
                && self
                    .output_lifecycles
                    .get(&output)
                    .and_then(LiveProductionMirrorGroupLifecycle::logically_submitted_frame)
                    .is_none()
            {
                return None;
            }
            let event = crate::LivePageFlipEvent {
                status: if completed_serial.is_some() {
                    crate::LivePageFlipEventStatus::Presented
                } else {
                    crate::LivePageFlipEventStatus::WaitingForOutput
                },
                frame_serial: completed_serial,
            };
            runtime.set_page_flip_observation(event);
            Some(event)
        }

        fn finish_mirror_presentation_cohort(
            &mut self,
            output: OutputId,
            frame: LiveProductionNativeFrameId,
        ) -> Result<(), Box<dyn std::error::Error>> {
            if !self
                .output_cohorts
                .get(&(output, frame))
                .is_some_and(|cohort| {
                    matches!(
                        cohort.terminal(),
                        Some(sophia_engine::OutputPresentationTerminal::Presented { .. })
                    )
                })
            {
                return Err("mirror generation joined before its Engine cohort presented".into());
            }
            Ok(())
        }

        fn run_mirror_group_scene_tick(
            &mut self,
            output: OutputId,
            runtime: &mut crate::LiveBackendRuntimeAssembly,
            input: CompositorBackendTickInput,
        ) -> Result<crate::LiveBackendRuntimeTickReport, Box<dyn std::error::Error>> {
            let indices = self.head_indices(output);
            if indices.is_empty() {
                return Err("mirror group has no head".into());
            }
            let retirement = self.service_mirror_group_retirement(output, runtime);
            if !retirement.errors.is_empty() {
                return Err(format!(
                    "mirror retirement failed after servicing callbacks and cleanup: {}",
                    retirement.errors.join("; ")
                )
                .into());
            }
            // Check stalls only after consuming callbacks that may have arrived
            // at the deadline. Drain-only retirement deliberately omits this
            // scheduler watchdog and relies on its outer bounded timeout.
            self.ensure_page_flip_progress()?;
            // Publish a completed join before the fallible Engine tick. A tick
            // failure must not erase the already-retired callback evidence.
            // Waiting is published later because this tick may start a new group.
            let completed_page_flip = retirement.completed_serial.and_then(|serial| {
                self.publish_mirror_group_page_flip(output, runtime, Some(serial))
            });

            // Advance the logical Engine exactly once. Physical callback owners
            // were retired first, so an Engine failure cannot consume and lose
            // their callback evidence.
            let mut tick = runtime.run_tick(input)?;
            tick.page_flip_callbacks = retirement.page_flip_callbacks;
            if let Some(event) = completed_page_flip {
                tick.page_flip = event;
            }
            let completed_retire = retirement.completed_retire;
            let completed_serial = retirement.completed_serial;
            if let Some(completed_serial) = completed_serial {
                self.finish_mirror_presentation_cohort(
                    output,
                    LiveProductionNativeFrameId::from_raw(completed_serial),
                )?;
            }

            for head_index in indices.iter().copied() {
                if self
                    .output_lifecycles
                    .get(&output)
                    .is_some_and(LiveProductionMirrorGroupLifecycle::failed)
                {
                    // Poison forbids new commits, not ownership cleanup. Visit
                    // every head so a failed later commit cannot strand the
                    // earlier head's retired framebuffer.
                    continue;
                }
                let head_id = self.heads[head_index].head;
                if self.heads[head_index].scanout_submission.is_some() {
                    self.heads[head_index].scanout_in_flight_ticks = self.heads[head_index]
                        .scanout_in_flight_ticks
                        .saturating_add(1);
                }
                if self.heads[head_index].scanout_cleanup.is_some() {
                    continue;
                }
                let already_prepared = self.heads[head_index].prepared_scanout.is_some();
                if !already_prepared && !self.exporters[head_index].pending_frame() {
                    continue;
                }
                let worker_was_in_flight = if already_prepared {
                    self.heads[head_index].prepared_worker_was_in_flight
                } else {
                    self.exporters[head_index].worker_in_flight()
                };
                let work_frame = if already_prepared {
                    self.heads[head_index].prepared_group_frame
                } else {
                    live_production_mirror_head_work_frame(
                        worker_was_in_flight,
                        self.heads[head_index].rendering_content,
                        self.heads[head_index].pending_content,
                    )
                }
                .ok_or("mirror head has renderer work without frame identity")?;
                let newest_frame = self
                    .output_lifecycles
                    .get(&output)
                    .and_then(LiveProductionMirrorGroupLifecycle::active_frame)
                    .ok_or("mirror renderer work has no ready generation")?;
                let logical_frame = work_frame;
                let selection = self.heads[head_index].selection;
                let size = self.heads[head_index].output.size;
                let head_group = self.heads[head_index].group;
                let submit = if let Some(prepared) = self.heads[head_index].prepared_scanout.take()
                {
                    if logical_frame != newest_frame {
                        let worker_owned = self.heads[head_index].prepared_worker_was_in_flight;
                        self.cancel_prepared_head_owner(head_index, prepared);
                        if worker_owned {
                            self.heads[head_index].rendering_content = None;
                            self.heads[head_index].output_frames.discard_rendering();
                        }
                        if let Some(cohort) = self.output_cohorts.get_mut(&(output, logical_frame))
                        {
                            let _ = cohort.mark_skipped(head_id);
                        }
                        continue;
                    }
                    if self.heads[head_index].scanout_submission.is_some()
                        || self.heads[head_index].scanout_cleanup.is_some()
                    {
                        self.heads[head_index].prepared_scanout = Some(prepared);
                        continue;
                    }
                    if !self
                        .output_cohorts
                        .get(&(output, logical_frame))
                        .is_some_and(sophia_engine::OutputPresentationCohort::all_prepared)
                    {
                        self.heads[head_index].prepared_scanout = Some(prepared);
                        continue;
                    }
                    let mut result = crate::submit_prepared_rendered_primary_plane_scanout(
                        self.groups[head_group].session.card(),
                        prepared,
                    );
                    if let Some(submission) = result.submission.take() {
                        self.heads[head_index].scanout_submission = Some(
                            submission
                                .with_submitted_after_page_flip_serial(
                                    self.heads[head_index].last_callback_serial,
                                )
                                .map_scanout_buffer(|owner| {
                                    Box::new(owner) as Box<dyn std::any::Any>
                                }),
                        );
                    }
                    if let Some(cleanup) = result.cleanup.take() {
                        self.heads[head_index].scanout_cleanup =
                            Some(cleanup.map_scanout_buffer(|owner| {
                                Box::new(owner) as Box<dyn std::any::Any>
                            }));
                    }
                    mirror_tracked_submit_report(&result, size)
                } else {
                    if !self.output_cohorts.contains_key(&(output, logical_frame)) {
                        let primary = self
                            .output_lifecycles
                            .get(&output)
                            .map(LiveProductionMirrorGroupLifecycle::primary_head)
                            .ok_or("mirror generation has no configured primary head")?;
                        let cohort = sophia_engine::OutputPresentationCohort::new(
                            output,
                            logical_frame.raw(),
                            primary,
                            indices.iter().map(|index| self.heads[*index].head),
                        )
                        .ok_or("mirror generation could not create a preparation cohort")?;
                        self.output_cohorts.insert((output, logical_frame), cohort);
                    }
                    // Each mirror head carries its own cursor contribution:
                    // the pointer projects differently per head, and a head
                    // it is not on hides in this same commit.
                    let cursor_ride = self.arm_cursor_ride(head_index);
                    if let Some((_, placement)) = cursor_ride {
                        self.heads[head_index].prepared_cursor_ride = Some(placement);
                    }
                    let mut prepare = {
                        let device = self.groups[head_group].session.card();
                        let exporter = &mut self.exporters[head_index];
                        crate::prepare_rendered_primary_plane_scanout_from_target_and_selection_with_cursor(
                            crate::LiveKmsScanoutTargetStatus::Ready,
                            Some(crate::LiveGbmEglFrameTargetRecord::new(size)),
                            crate::LibdrmNativePrimaryPlaneSelectionResult {
                                status: crate::LibdrmNativePrimaryPlaneSelectionStatus::Selected,
                                selection: Some(selection),
                            },
                            None,
                            cursor_ride.map(|(cursor, _)| cursor),
                            device,
                            exporter,
                        )
                    };
                    let report = mirror_tracked_prepare_report(&prepare, size);
                    if let Some(cleanup) = prepare.cleanup.take() {
                        self.heads[head_index].scanout_cleanup =
                            Some(cleanup.map_scanout_buffer(|owner| {
                                Box::new(owner) as Box<dyn std::any::Any>
                            }));
                    }
                    if let Some(prepared) = prepare.prepared.take() {
                        if logical_frame != newest_frame {
                            self.cancel_prepared_head_owner(head_index, prepared);
                            if worker_was_in_flight {
                                self.heads[head_index].rendering_content = None;
                                self.heads[head_index].output_frames.discard_rendering();
                            }
                            if let Some(cohort) =
                                self.output_cohorts.get_mut(&(output, logical_frame))
                            {
                                let _ = cohort.mark_skipped(head_id);
                            }
                            tracing::info!(
                                "sophia_live_mirror_pacing schema=1 status=coalesced output={} head={} skipped={} newest={}",
                                output.raw(),
                                head_id.raw(),
                                logical_frame.raw(),
                                newest_frame.raw(),
                            );
                            continue;
                        }
                        let content = if worker_was_in_flight {
                            self.heads[head_index].rendering_content
                        } else {
                            self.heads[head_index].pending_content
                        };
                        let Some(content) = content else {
                            self.cancel_prepared_head_owner(head_index, prepared);
                            return Err("prepared mirror head lost its content identity".into());
                        };
                        let logical_content_checksum = content
                            .cpu_checksum()
                            .unwrap_or(self.heads[head_index].last_checksum);
                        let candidate = sophia_engine::HeadFrameCandidate {
                            candidate: self.allocate_head_candidate_id(),
                            output,
                            scene_generation: logical_frame.raw(),
                            head: head_id,
                            target_generation: self.heads[head_index].target_generation,
                            logical_content_checksum,
                        };
                        let transition = self
                            .output_cohorts
                            .get_mut(&(output, logical_frame))
                            .expect("mirror preparation cohort exists")
                            .mark_prepared(candidate);
                        if !matches!(
                            transition,
                            sophia_engine::OutputPresentationTransition::Accepted
                                | sophia_engine::OutputPresentationTransition::PhaseReady
                        ) {
                            self.cancel_prepared_head_owner(head_index, prepared);
                            return Err(format!(
                                "mirror head {} entered invalid prepared transition {transition:?}",
                                head_id.raw(),
                            )
                            .into());
                        }
                        self.heads[head_index].prepared_scanout = Some(prepared);
                        self.heads[head_index].prepared_group_frame = Some(logical_frame);
                        self.heads[head_index].prepared_worker_was_in_flight = worker_was_in_flight;
                        self.heads[head_index].last_submit_report = Some(report);
                        self.output_lifecycles
                            .get_mut(&output)
                            .expect("mirror output has a lifecycle")
                            .observe_physical_progress(logical_frame);
                        tracing::trace!(
                            "sophia_live_native_head_page_flip schema=2 status=prepared output={} head={} frame={} all_prepared={}",
                            output.raw(),
                            head_id.raw(),
                            logical_frame.raw(),
                            self.output_cohorts
                                .get(&(output, logical_frame))
                                .is_some_and(sophia_engine::OutputPresentationCohort::all_prepared),
                        );
                        if tick.rendered_primary_plane_scanout_submit.is_none() {
                            tick.rendered_primary_plane_scanout_submit = Some(report);
                        }
                        continue;
                    }
                    report
                };
                self.heads[head_index].last_submit_report = Some(submit);
                use crate::LiveTrackedRenderedPrimaryPlaneScanoutSubmitStatus as Status;
                let worker_is_in_flight = self.exporters[head_index].worker_in_flight();
                let renderer_started = {
                    let head = &mut self.heads[head_index];
                    advance_live_production_renderer_content(
                        worker_was_in_flight,
                        worker_is_in_flight,
                        &mut head.pending_content,
                        &mut head.rendering_content,
                    )?
                };
                if renderer_started && self.heads[head_index].output_frames.pending().is_some() {
                    self.heads[head_index]
                        .output_frames
                        .mark_rendering()
                        .map_err(|error| {
                            format!("mirror display-list render transition failed: {error}")
                        })?;
                    let progressed = self
                        .output_lifecycles
                        .get_mut(&output)
                        .expect("mirror output has a lifecycle")
                        .observe_physical_progress(logical_frame);
                    debug_assert!(progressed);
                }
                match submit.status {
                    Status::SubmittedWaitingForPageFlip => {
                        self.heads[head_index].prepared_group_frame = None;
                        self.heads[head_index].prepared_worker_was_in_flight = false;
                        let pending_before = self.heads[head_index].pending_content;
                        let rendering_before = self.heads[head_index].rendering_content;
                        let content = if worker_was_in_flight {
                            self.heads[head_index].rendering_content.take()
                        } else {
                            self.heads[head_index].pending_content.take()
                        };
                        if content.is_none() {
                            return Err(format!(
                                "accepted mirror submission lost its content identity: \
output={} head={} selected_slot={} worker_was_in_flight={} worker_is_in_flight={} \
pending_before={pending_before:?} rendering_before={rendering_before:?} exporter_pending={}",
                                output.raw(),
                                head_id.raw(),
                                if worker_was_in_flight {
                                    "rendering"
                                } else {
                                    "pending"
                                },
                                worker_was_in_flight,
                                worker_is_in_flight,
                                self.exporters[head_index].pending_frame(),
                            )
                            .into());
                        }
                        let content = content.map(|content| {
                            content.with_nonzero_rgb_pixels(
                                self.exporters[head_index].composition_nonzero_rgb_pixels(),
                            )
                        });
                        if worker_was_in_flight
                            && self.heads[head_index].output_frames.rendering().is_some()
                        {
                            self.heads[head_index]
                                .output_frames
                                .promote_rendering_to_submitted()
                                .map_err(|error| {
                                    format!("mirror display-list worker promotion failed: {error}")
                                })?;
                        } else if !worker_was_in_flight
                            && self.heads[head_index].output_frames.pending().is_some()
                        {
                            self.heads[head_index]
                                .output_frames
                                .mark_submitted()
                                .map_err(|error| {
                                    format!("mirror display-list submit failed: {error}")
                                })?;
                        }
                        self.heads[head_index].submitted_content = content;
                        self.heads[head_index].submitted_group_frame = Some(logical_frame);
                        self.heads[head_index].submissions =
                            self.heads[head_index].submissions.saturating_add(1);
                        self.heads[head_index].submitted_sequence =
                            Some(self.heads[head_index].submissions);
                        self.heads[head_index].submitted_checksum = Some(
                            content
                                .and_then(LiveProductionScanoutContent::cpu_checksum)
                                .unwrap_or(self.heads[head_index].last_checksum),
                        );
                        self.heads[head_index].submitted_at = Some(Instant::now());
                        if let Some(placement) = self.heads[head_index].prepared_cursor_ride.take()
                        {
                            if submit.cursor_dropped {
                                self.cursor_combined_drops =
                                    self.cursor_combined_drops.saturating_add(1);
                            } else {
                                self.settle_atomic_cursor(head_index, placement, true);
                            }
                        }
                        self.heads[head_index].submitted_ust_usec =
                            Some(Self::monotonic_ust_usec());
                        self.submissions = self.submissions.saturating_add(1);
                        let exported_nonzero =
                            matches!(content, Some(LiveProductionScanoutContent::Cpu { .. }))
                                && self.heads[head_index].pending_nonzero_pixel_bytes > 0
                                || matches!(
                                    content,
                                    Some(
                                        LiveProductionScanoutContent::MixedPresent {
                                            nonzero_rgb_pixels: 1..,
                                            ..
                                        } | LiveProductionScanoutContent::RetainedMixed {
                                            nonzero_rgb_pixels: 1..,
                                            ..
                                        } | LiveProductionScanoutContent::HeadComposition {
                                            nonzero_rgb_pixels: 1..,
                                            ..
                                        }
                                    )
                                );
                        if exported_nonzero {
                            self.nonzero_exports = self.nonzero_exports.saturating_add(1);
                            self.heads[head_index].nonzero_exports =
                                self.heads[head_index].nonzero_exports.saturating_add(1);
                        }
                        if matches!(content, Some(LiveProductionScanoutContent::Cpu { .. })) {
                            self.heads[head_index].pending_nonzero_pixel_bytes = 0;
                        }
                        tracing::trace!(
                            "sophia_live_native_head_page_flip schema=2 status=submitted output={} head={} submission={} content={:?} frame={}",
                            output.raw(),
                            head_id.raw(),
                            self.heads[head_index].submissions,
                            content,
                            logical_frame.raw(),
                        );
                        let cohort_transition = self
                            .output_cohorts
                            .get_mut(&(output, logical_frame))
                            .ok_or("submitted mirror generation has no preparation cohort")?
                            .mark_submitted(head_id);
                        if !matches!(
                            cohort_transition,
                            sophia_engine::OutputPresentationTransition::Accepted
                                | sophia_engine::OutputPresentationTransition::PhaseReady
                        ) {
                            return Err(format!(
                                "mirror-head {} entered invalid cohort submit transition {cohort_transition:?}",
                                head_id.raw(),
                            )
                            .into());
                        }
                        let transition = self
                            .output_lifecycles
                            .get_mut(&output)
                            .expect("mirror output has a lifecycle")
                            .mark_submitted(head_id, logical_frame);
                        match transition {
                            LiveProductionMirrorHeadTransition::GroupReady => {
                                let cycle = logical_frame.raw();
                                if let Err(error) = self.production_page_flips.submit(output, cycle)
                                {
                                    self.vsync_overlap_rejections =
                                        self.vsync_overlap_rejections.saturating_add(1);
                                    return Err(format!(
                                        "mirror logical page-flip submission was rejected: {error:?}"
                                    )
                                    .into());
                                }
                                tick.rendered_primary_plane_scanout_submit = Some(submit);
                            }
                            LiveProductionMirrorHeadTransition::Accepted => {}
                            invalid => {
                                return Err(format!(
                                    "mirror-head {} entered invalid submitted transition {invalid:?}",
                                    head_id.raw(),
                                )
                                .into());
                            }
                        }
                    }
                    Status::ScanoutExportPending => {
                        self.submit_deferred = self.submit_deferred.saturating_add(1);
                        // The logical Present owns this generation as soon as any
                        // physical exporter starts. Returning `None` leaves the
                        // Present queued and lets the next Ready pass replace the
                        // frame whose worker is still running.
                        if tick.rendered_primary_plane_scanout_submit.is_none() {
                            tick.rendered_primary_plane_scanout_submit = Some(submit);
                        }
                    }
                    Status::AlreadyInFlight | Status::CleanupPending => {
                        self.submit_deferred = self.submit_deferred.saturating_add(1);
                    }
                    _ => {
                        if worker_was_in_flight {
                            self.heads[head_index].output_frames.discard_rendering();
                            self.heads[head_index].rendering_content = None;
                        } else {
                            self.heads[head_index].output_frames.discard_pending();
                            self.heads[head_index].pending_content = None;
                        }
                        self.submit_failures = self.submit_failures.saturating_add(1);
                        let cohort_failure = if self
                            .output_cohorts
                            .get(&(output, logical_frame))
                            .is_some_and(sophia_engine::OutputPresentationCohort::all_prepared)
                        {
                            sophia_engine::OutputPresentationFailure::Submission
                        } else {
                            sophia_engine::OutputPresentationFailure::Preparation
                        };
                        if let Some(cohort) = self.output_cohorts.get_mut(&(output, logical_frame))
                        {
                            cohort.fail(cohort_failure);
                        }
                        for prepared_index in indices.iter().copied() {
                            if let Some(prepared) =
                                self.heads[prepared_index].prepared_scanout.take()
                            {
                                self.cancel_prepared_head_owner(prepared_index, prepared);
                            }
                        }
                        tracing::error!(
                            "sophia_live_native_head_page_flip schema=2 status=submit_failed output={} head={} submit_status={:?} action=terminate_session",
                            output.raw(),
                            head_id.raw(),
                            submit.status,
                        );
                        let aborted = self
                            .output_lifecycles
                            .get_mut(&output)
                            .expect("mirror output has a lifecycle")
                            .abort(logical_frame);
                        if !aborted {
                            return Err(
                                "mirror submit failure could not poison its generation".into()
                            );
                        }
                        break;
                    }
                }
            }
            if self
                .output_lifecycles
                .get(&output)
                .is_some_and(|lifecycle| {
                    lifecycle.active_generation_hard_stalled(LIVE_PRODUCTION_PAGE_FLIP_HARD_STALL)
                })
            {
                let blockers = indices
                    .iter()
                    .map(|index| {
                        let head = &self.heads[*index];
                        format!(
                            "head={} kms={} cleanup={} worker={} pending={:?} rendering={:?} newest={:?}",
                            head.head.raw(),
                            head.scanout_submission.is_some(),
                            head.scanout_cleanup.is_some(),
                            self.exporters[*index].worker_in_flight(),
                            head.pending_content.map(LiveProductionScanoutContent::frame),
                            head.rendering_content.map(LiveProductionScanoutContent::frame),
                            self.output_lifecycles
                                .get(&output)
                                .and_then(LiveProductionMirrorGroupLifecycle::active_frame),
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("; ");
                return Err(format!(
                    "mirror group generation made no physical progress within {:?}: output={} active={:?} blockers=[{}]",
                    LIVE_PRODUCTION_PAGE_FLIP_HARD_STALL,
                    output.raw(),
                    self.output_lifecycles
                        .get(&output)
                        .and_then(LiveProductionMirrorGroupLifecycle::active_frame),
                    blockers,
                )
                .into());
            }
            tick.rendered_primary_plane_scanout_retire = completed_retire;
            if completed_page_flip.is_none()
                && let Some(logical_page_flip) =
                    self.publish_mirror_group_page_flip(output, runtime, completed_serial)
            {
                tick.page_flip = logical_page_flip;
            }
            tick.rendered_primary_plane_scanout_cleanup_pending = indices
                .iter()
                .any(|index| self.heads[*index].scanout_cleanup.is_some());
            tick.rendered_primary_plane_scanout_in_flight_ticks = self
                .heads
                .iter()
                .enumerate()
                .filter(|(index, _)| indices.contains(index))
                .map(|(_, head)| head.scanout_in_flight_ticks)
                .max()
                .unwrap_or_default();
            Ok(tick)
        }

        pub fn retire_ready(
            &mut self,
            output: OutputId,
            runtime: &mut crate::LiveBackendRuntimeAssembly,
        ) -> Result<(), Box<dyn std::error::Error>> {
            if self.head_indices(output).len() > 1 {
                // Retirement is physical ownership work. In particular it must
                // not invent an empty compositor input and re-enter scene
                // projection: an output can have committed surfaces while a
                // page-flip poll has no layer templates to contribute. The frame
                // service will schedule any promoted successor through `run_tick`
                // after this callback-only phase returns.
                let retirement = self.service_mirror_group_retirement(output, runtime);
                if !retirement.errors.is_empty() {
                    return Err(format!(
                        "mirror retirement failed after servicing callbacks and cleanup: {}",
                        retirement.errors.join("; ")
                    )
                    .into());
                }
                self.publish_mirror_group_page_flip(output, runtime, retirement.completed_serial);
                if let Some(completed_serial) = retirement.completed_serial {
                    self.finish_mirror_presentation_cohort(
                        output,
                        LiveProductionNativeFrameId::from_raw(completed_serial),
                    )?;
                }
                if self.mirror_poison_drained(output) {
                    return Err("mirror generation failed after physical ownership drained".into());
                }
                return Ok(());
            }
            let index = self.primary_head(output)?;
            let group = self.heads[index].group;
            let mut callbacks = crate::LivePageFlipCallbackQueueReport::with_accepted_capacity(1);
            let completion = self.heads[index]
                .pending_callback
                .take()
                .map(|callback| (callback, LiveProductionKmsCompletionSource::PageFlipEvent));
            let completion = if completion.is_some() {
                completion
            } else {
                match runtime.rendered_primary_plane_completion_fence_status_for(output) {
                    Ok(status) => {
                        self.heads[index].completion_fence_status = status;
                        if status == crate::LibdrmNativeCompletionFenceStatus::Signaled {
                            Some((
                                self.synthesize_out_fence_callback(index),
                                LiveProductionKmsCompletionSource::OutFence,
                            ))
                        } else {
                            None
                        }
                    }
                    Err(error) => {
                        self.heads[index].completion_fence_errors =
                            self.heads[index].completion_fence_errors.saturating_add(1);
                        return Err(format!("native completion fence poll failed: {error}").into());
                    }
                }
            };
            let mut completion_source = LiveProductionKmsCompletionSource::PageFlipEvent;
            let retire = completion.and_then(|(callback, source)| {
                completion_source = source;
                let observation = runtime.observe_page_flip_callback(callback);
                callbacks.record_observation(callback, observation);
                (observation.decision == crate::LivePageFlipCallbackDecision::Accepted).then(|| {
                    runtime.retire_tracked_rendered_primary_plane_scanout_after_page_flip(
                        self.groups[group].session.card(),
                        &observation,
                    )
                })
            });
            if let Some(retire) = retire {
                self.observe_retire(index, retire);
            }
            self.observe_callbacks_with_source(index, callbacks, completion_source);
            Ok(())
        }

        pub(crate) fn retire_ready_for_drain(
            &mut self,
            output: OutputId,
            runtime: &mut crate::LiveBackendRuntimeAssembly,
        ) -> Result<(), Box<dyn std::error::Error>> {
            self.invalidate_layout_probes();
            self.service_layout_probe_cleanup();
            if self.head_indices(output).len() > 1 {
                let retirement = self.service_mirror_group_retirement(output, runtime);
                if !retirement.errors.is_empty() {
                    return Err(format!(
                        "mirror drain failed after servicing callbacks and cleanup: {}",
                        retirement.errors.join("; ")
                    )
                    .into());
                }
                self.publish_mirror_group_page_flip(output, runtime, retirement.completed_serial);
                return Ok(());
            }
            self.retire_ready(output, runtime)
        }

        pub fn retire_ready_and_retry_cleanup(
            &mut self,
            output: OutputId,
            runtime: &mut crate::LiveBackendRuntimeAssembly,
        ) -> Result<(), Box<dyn std::error::Error>> {
            self.service_layout_probe_cleanup();
            let index = self.primary_head(output)?;
            self.retire_ready(output, runtime)?;
            if runtime.rendered_primary_plane_scanout_cleanup_pending() {
                let cleanup =
                    runtime.retry_tracked_rendered_primary_plane_scanout_cleanup(self.card(index));
                if !cleanup.cleanup_pending {
                    self.retire_failures = self.retire_failures.saturating_sub(1);
                }
            }
            Ok(())
        }

        fn cancel_prepared_head_owner(
            &mut self,
            head_index: usize,
            prepared: crate::LivePreparedRenderedPrimaryPlaneScanout<
                crate::NativeGbmRenderedScanoutOwner,
            >,
        ) {
            let group = self.heads[head_index].group;
            let result = crate::cancel_prepared_rendered_primary_plane_scanout(
                self.groups[group].session.card(),
                prepared,
            );
            if let Some(cleanup) = result.cleanup {
                self.heads[head_index].scanout_cleanup = Some(
                    cleanup.map_scanout_buffer(|owner| Box::new(owner) as Box<dyn std::any::Any>),
                );
            }
            if result.destroy != crate::LibdrmNativePrimaryPlaneResourceDestroyStatus::Destroyed {
                self.retire_failures = self.retire_failures.saturating_add(1);
            }
            self.heads[head_index].prepared_group_frame = None;
            self.heads[head_index].prepared_worker_was_in_flight = false;
        }

        pub fn release_displayed_output(
            &mut self,
            output: OutputId,
            runtime: &mut crate::LiveBackendRuntimeAssembly,
        ) -> Result<(), Box<dyn std::error::Error>> {
            let index = self.primary_head(output)?;
            self.invalidate_layout_probes();
            self.service_layout_probe_cleanup();
            trace_live_native_lifecycle("displayed_scanout_retire_started");
            let retired = runtime.retire_displayed_rendered_primary_plane_scanout(self.card(index));
            let mut runtime_cleanup_pending = retired.cleanup_pending;
            if retired.cleanup_pending {
                trace_live_native_lifecycle("displayed_scanout_cleanup_retry_started");
                let cleanup =
                    runtime.retry_tracked_rendered_primary_plane_scanout_cleanup(self.card(index));
                runtime_cleanup_pending = cleanup.cleanup_pending;
            }
            let mut mirror_cleanup_pending = false;
            for head_index in self.head_indices(output) {
                if let Some(prepared) = self.heads[head_index].prepared_scanout.take() {
                    self.cancel_prepared_head_owner(head_index, prepared);
                }
                if let Some(displayed) = self.heads[head_index].displayed_scanout.take() {
                    let crate::LiveRenderedPrimaryPlaneScanoutSubmission {
                        scanout_buffer,
                        primary_plane,
                        ..
                    } = displayed;
                    let released = primary_plane.retire(self.card(head_index));
                    if let Some(primary_plane) = released.cleanup {
                        self.heads[head_index].scanout_cleanup =
                            Some(crate::LiveRenderedPrimaryPlaneScanoutCleanup {
                                scanout_buffer,
                                primary_plane,
                            });
                    }
                }
                if let Some(cleanup) = self.heads[head_index].scanout_cleanup.take() {
                    let retried = crate::retry_rendered_primary_plane_scanout_cleanup(
                        self.card(head_index),
                        cleanup,
                    );
                    self.heads[head_index].scanout_cleanup = retried.cleanup;
                }
                if self.heads[head_index].scanout_cleanup.is_some() {
                    mirror_cleanup_pending = true;
                }
            }
            if runtime_cleanup_pending
                || mirror_cleanup_pending
                || self.layout_probe_cleanup_pending()
            {
                return Err(format!(
                    "persistent displayed scanout cleanup remained pending: runtime={} mirror_heads={}",
                    runtime_cleanup_pending, mirror_cleanup_pending,
                )
                .into());
            }
            trace_live_native_lifecycle("displayed_scanout_owner_released");
            self.deferred_mirror_generations.remove(&output);
            for ((cohort_output, frame), _) in self
                .output_cohorts
                .iter()
                .filter(|((cohort_output, _), _)| *cohort_output == output)
            {
                tracing::info!(
                    "sophia_live_mirror_pacing schema=1 status=released output={} frame={}",
                    cohort_output.raw(),
                    frame.raw(),
                );
            }
            self.output_cohorts
                .retain(|(cohort_output, _), _| *cohort_output != output);
            for head_index in self.head_indices(output) {
                self.heads[head_index].displayed_group_frame = None;
                self.heads[head_index].scanout_cleanup_group_frame = None;
            }
            Ok(())
        }

        pub fn cancel_prepared_output(&mut self, output: OutputId) -> usize {
            let mut cancelled = 0usize;
            for head_index in self.head_indices(output) {
                let Some(prepared) = self.heads[head_index].prepared_scanout.take() else {
                    continue;
                };
                self.cancel_prepared_head_owner(head_index, prepared);
                cancelled = cancelled.saturating_add(1);
            }
            if cancelled > 0 {
                for cohort in
                    self.output_cohorts
                        .iter_mut()
                        .filter_map(|((cohort_output, _), cohort)| {
                            (*cohort_output == output).then_some(cohort)
                        })
                {
                    cohort.fail(sophia_engine::OutputPresentationFailure::StaleTopology);
                }
                tracing::info!(
                    "sophia_live_mirror_generation schema=2 status=preparation_cancelled output={} heads={cancelled}",
                    output.raw(),
                );
            }
            cancelled
        }

        pub fn observe_retire(
            &mut self,
            index: usize,
            retire: crate::LiveTrackedRenderedPrimaryPlaneScanoutRetireReport,
        ) {
            if retire.status == crate::LiveTrackedRenderedPrimaryPlaneScanoutRetireStatus::HeadLost
            {
                self.invalidate_layout_probes();
            }
            self.observe_layout_witness_retire(index, retire);
            use crate::LiveTrackedRenderedPrimaryPlaneScanoutRetireStatus as Status;
            match retire.status {
                Status::RetiredAfterPageFlip => {
                    trace_live_native_lifecycle("kms_buffer_retired");
                    let frame = self.heads[index]
                        .submitted_content
                        .or(self.heads[index].presented_content)
                        .map_or(0, |content| content.frame().raw());
                    tracing::trace!(
                        "sophia_live_native_page_flip schema=1 status=retired output={} submission={} frame={}",
                        self.heads[index].output.id.raw(),
                        self.heads[index]
                            .submitted_sequence
                            .unwrap_or(self.heads[index].submissions),
                        frame,
                    );
                    trace_native_head_retirement(
                        self.heads[index].output.id.raw(),
                        self.heads[index].head.raw(),
                        self.heads[index]
                            .submitted_sequence
                            .unwrap_or(self.heads[index].submissions),
                        frame,
                    );
                    self.retirements = self.retirements.saturating_add(1);
                    self.heads[index].retirements = self.heads[index].retirements.saturating_add(1);
                }
                Status::HeadLost => {
                    trace_live_native_lifecycle("kms_buffer_released_after_head_loss");
                    tracing::warn!(
                        "sophia_live_native_page_flip schema=1 status=head_lost output={}",
                        self.heads[index].output.id.raw(),
                    );
                }
                Status::NoSubmission | Status::WaitingForAcceptedPageFlip => {}
                Status::ResourceRetireFailed => {
                    self.retire_failures = self.retire_failures.saturating_add(1);
                }
            }
        }

        pub fn observe_callbacks(
            &mut self,
            index: usize,
            report: crate::LivePageFlipCallbackQueueReport,
        ) {
            self.observe_callbacks_with_source(
                index,
                report,
                LiveProductionKmsCompletionSource::PageFlipEvent,
            );
        }

        fn observe_callbacks_with_source(
            &mut self,
            index: usize,
            report: crate::LivePageFlipCallbackQueueReport,
            completion_source: LiveProductionKmsCompletionSource,
        ) {
            self.callback_accepted = self.callback_accepted.saturating_add(report.accepted);
            self.heads[index].callback_accepted = self.heads[index]
                .callback_accepted
                .saturating_add(report.accepted);
            if report.accepted > 0 {
                trace_live_native_lifecycle("page_flip_callback_accepted");
                if completion_source == LiveProductionKmsCompletionSource::PageFlipEvent {
                    tracing::trace!(
                        "sophia_live_native_page_flip schema=1 status=callback_accepted output={} callbacks={} kernel_sequence={}",
                        self.heads[index].output.id.raw(),
                        report.accepted,
                        report
                            .last_accepted
                            .and_then(|accepted| accepted.event.frame_serial)
                            .map_or_else(|| "none".to_owned(), |serial| serial.to_string()),
                    );
                    tracing::trace!(
                        "sophia_live_native_head_page_flip schema=2 status=callback_accepted output={} head={} callbacks={} kernel_sequence={}",
                        self.heads[index].output.id.raw(),
                        self.heads[index].head.raw(),
                        report.accepted,
                        report
                            .last_accepted
                            .and_then(|accepted| accepted.event.frame_serial)
                            .map_or_else(|| "none".to_owned(), |serial| serial.to_string()),
                    );
                } else {
                    tracing::info!(
                        "sophia_live_native_completion schema=1 status=accepted output={} head={} callbacks={} completion_source={} completion_serial={}",
                        self.heads[index].output.id.raw(),
                        self.heads[index].head.raw(),
                        report.accepted,
                        completion_source.label(),
                        report
                            .last_accepted
                            .and_then(|accepted| accepted.event.frame_serial)
                            .map_or_else(|| "none".to_owned(), |serial| serial.to_string()),
                    );
                }
                self.heads[index].last_callback_serial = report
                    .last_accepted
                    .and_then(|accepted| accepted.event.frame_serial);
                if let Some(checksum) = self.heads[index].submitted_checksum.take() {
                    self.heads[index].presented_logical_checksum = checksum;
                }
                if let Some(submission) = self.heads[index].submitted_sequence.take() {
                    self.heads[index].presented_submissions = submission;
                }
                self.heads[index].presented_content = self.heads[index].submitted_content.take();
                self.heads[index].presented_direct =
                    std::mem::take(&mut self.heads[index].submitted_direct);
                if self.heads[index].output_frames.submitted().is_some() {
                    let presented = self.heads[index]
                        .output_frames
                        .mark_presented()
                        .expect("submitted display-list state checked above");
                    trace_presented_output_damage(
                        "presented",
                        self.heads[index].output.id,
                        &presented,
                    );
                }
                let output = self.heads[index].output.id;
                if let Some(kernel_sequence) = report
                    .last_accepted
                    .and_then(|accepted| accepted.event.frame_serial)
                {
                    let ust = self.completion_ust_usec(
                        output,
                        self.heads[index].head,
                        kernel_sequence,
                        completion_source,
                    );
                    let submitted_ust_usec = self.heads[index].submitted_ust_usec.take();
                    let submit_to_page_flip = submitted_ust_usec
                        .and_then(|submitted| ust.checked_sub(submitted))
                        .map(Duration::from_micros)
                        .or_else(|| {
                            self.heads[index]
                                .submitted_at
                                .map(|submitted| submitted.elapsed())
                        })
                        .unwrap_or_default();
                    self.heads[index].submitted_at = None;
                    self.max_submit_to_page_flip =
                        self.max_submit_to_page_flip.max(submit_to_page_flip);
                    self.heads[index].presented_submission_ust_usec =
                        submitted_ust_usec.unwrap_or_default();
                    self.heads[index].presented_page_flip_ust_usec = ust;
                    self.heads[index].presented_submit_to_page_flip = submit_to_page_flip;
                    // What the display engine did with the buffer, filed
                    // under how the buffer got there. This half should not
                    // differ by population, and is measured to find out
                    // rather than to assume.
                    self.cost.record_submit_to_flip(
                        self.heads[index].presented_direct,
                        submit_to_page_flip,
                    );
                    let native_content = self.completed_native_content(index);
                    if let Err(error) = self.production_page_flips.observe_native_page_flip(
                        output,
                        kernel_sequence,
                        ust,
                        native_content,
                    ) {
                        self.page_flip_phase_rejections =
                            self.page_flip_phase_rejections.saturating_add(1);
                        tracing::error!(
                            "sophia_live_native_pacing schema=1 status=completion_rejected output={} kernel_sequence={} completion_source={} ust_usec={} error={error:?}",
                            output.raw(),
                            kernel_sequence,
                            completion_source.label(),
                            ust,
                        );
                    }
                }
            }
            self.callback_rejected = self.callback_rejected.saturating_add(
                report.rejected_unexpected_output + report.rejected_stale_frame_serial,
            );
            self.callback_queue_saturated = self
                .callback_queue_saturated
                .saturating_add(usize::from(report.max_reached));
        }

        fn completion_ust_usec(
            &mut self,
            output: OutputId,
            head: sophia_engine::RenderHeadId,
            sequence: u64,
            source: LiveProductionKmsCompletionSource,
        ) -> u64 {
            // Always consume a matching timestamp record. An out-fence is
            // authoritative once selected, so a late kernel event must not
            // leave timing evidence resident after its physical owner retires.
            let kernel_ust = self.kernel_page_flip_ust.remove(&(output, head, sequence));
            let needs_fallback =
                source == LiveProductionKmsCompletionSource::OutFence || kernel_ust.is_none();
            let timestamp = reduce_live_production_completion_timestamp(
                source,
                kernel_ust,
                needs_fallback
                    .then(Self::monotonic_ust_usec)
                    .unwrap_or_default(),
            );
            if timestamp.used_kernel_timestamp {
                self.kernel_page_flip_timestamps =
                    self.kernel_page_flip_timestamps.saturating_add(1);
            }
            if timestamp.missing_kernel_timestamp {
                self.kernel_page_flip_timestamp_missing =
                    self.kernel_page_flip_timestamp_missing.saturating_add(1);
            }
            // Kernel page-flip UST and every fallback share the
            // CLOCK_MONOTONIC epoch. Session-relative elapsed time would jump
            // backward when a head changes to authoritative out-fence
            // completion and would strand the logical presentation owner.
            timestamp.ust_usec
        }

        fn monotonic_ust_usec() -> u64 {
            let now = rustix::time::clock_gettime(rustix::time::ClockId::Monotonic);
            let seconds = u64::try_from(now.tv_sec).unwrap_or_default();
            let nanos = u64::try_from(now.tv_nsec).unwrap_or_default();
            seconds
                .saturating_mul(1_000_000)
                .saturating_add(nanos / 1_000)
        }

        pub fn initialize_head_composition(
            &mut self,
            output: OutputId,
            runtime: &mut crate::LiveBackendRuntimeAssembly,
            frames: Vec<LiveProductionHeadCompositionFrame>,
        ) -> Result<LiveProductionNativeFrameId, Box<dyn std::error::Error>> {
            let has_head = !self.head_indices(output).is_empty();
            let initialized = self.initialize_semantic_head_transaction(output, runtime, frames);
            match initialized {
                Ok(frame) => Ok(frame),
                Err(error) => {
                    let error = match self.abort_semantic_startup_head_work(output) {
                        Ok(()) => error,
                        Err(abort) => format!(
                            "semantic startup failed: {error}; renderer abort failed: {abort}"
                        )
                        .into(),
                    };
                    finish_live_production_native_initialization(Err(error), has_head, || {
                        self.release_displayed_output(output, runtime)
                    })?;
                    unreachable!("failed initialization cannot settle successfully")
                }
            }
        }

        /// Whether outputs of one device group share a renderer thread.
        ///
        /// Opt-in until the shared worker is promoted on physical evidence.
        /// A head that renders alone cannot starve a sibling or misroute a
        /// result to one, so the failure modes this introduces do not exist
        /// until it is on, and the gate that proves them is the one that
        /// turns it on.
        fn shared_renderer_worker_enabled() -> bool {
            std::env::var("SOPHIA_ENABLE_SHARED_RENDERER_WORKER").is_ok_and(|value| value == "1")
        }

        /// Whether a head may hand an eligible client buffer straight to a
        /// plane instead of composing it.
        ///
        /// Opt-in until the row is promoted on physical evidence. Off, the
        /// exporter never even derives a candidate, so the session behaves
        /// exactly as it did before this row rather than taking a different
        /// path that happens to compose.
        fn direct_scanout_enabled() -> bool {
            std::env::var("SOPHIA_ENABLE_DIRECT_SCANOUT").is_ok_and(|value| value == "1")
        }

        /// Give one head a renderer worker: its group's shared thread when
        /// sharing is on, a thread of its own when it is not.
        ///
        /// Every path that brings a head up runs through here, because a head
        /// enabled the other way would quietly keep its own EGL display and
        /// its own copy of every imported image while the session reported
        /// itself as sharing.
        pub(crate) fn enable_head_renderer_worker(
            &mut self,
            index: usize,
        ) -> Result<(), Box<dyn std::error::Error>> {
            self.invalidate_layout_probes();
            // The head's own identity, so two exporters on one core never
            // collide in their replies, their slots, or their leases.
            // Group in the high bits, head in the low. Head identities repeat
            // across cards -- a two-card guest reports head=1 for both of its
            // outputs -- and while a key only has to be unique within the core
            // that holds it, uniqueness by construction beats uniqueness by an
            // argument about scope that a later change could quietly break.
            let group = u64::try_from(self.heads[index].group).unwrap_or(u64::MAX);
            self.exporters[index].set_output(crate::LiveRendererWorkerOutputKey::from_raw(
                (group << 32) | (self.heads[index].head.raw() & 0xFFFF_FFFF),
            ));
            // A mirror head never takes the direct path. Eligibility is proven
            // about one head's plan; a mirror cohort projects one scene into
            // several heads' own modes, so the buffer that would fill one head
            // exactly does not fill its siblings, and there is no single
            // client buffer that is the group's image. This is the first of
            // two refusals -- the mirror queue clears the verdict on the frame
            // itself -- because head membership can change after a head is
            // enabled, and neither check alone covers both orders.
            let mirrored = self.head_indices(self.heads[index].output.id).len() > 1;
            // Not yet, even when the session asked for it: `admit_direct_scanout`
            // turns it on once startup readiness has proven a picture reached
            // glass. A head enabled here would take the direct path before that
            // proof could be made, and the proof reads composed pixels.
            self.direct_scanout_admissible = Self::direct_scanout_enabled();
            self.exporters[index].set_direct_scanout_enabled(
                self.direct_scanout_admitted
                    && self.direct_scanout_admissible
                    && !mirrored
                    && !self.translation_motion_active,
            );
            if Self::shared_renderer_worker_enabled() {
                let group = self.heads[index].group;
                if self.groups[group].renderer_core.is_none() {
                    let discovery = self.groups[group].session.render_device_discovery()?;
                    self.groups[group].renderer_core = Some(
                        crate::NativeGbmRendererWorkerCore::spawn_with_image_import_devices(
                            crate::RenderDeviceDiscoveryBackend::open_render_device(&discovery),
                            self.image_import_device_fds()?,
                        )?,
                    );
                    self.render_devices.pending.remove(&group);
                    self.render_devices
                        .applied
                        .insert(group, self.render_devices.generation);
                }
                let core = self.groups[group]
                    .renderer_core
                    .as_ref()
                    .expect("group renderer core established above")
                    .clone();
                self.exporters[index].attach_shared_worker(&core);
            } else {
                let image_import_devices = self.image_import_device_fds()?;
                self.exporters[index]
                    .enable_worker_with_image_import_devices(image_import_devices)?;
                self.render_devices.pending.remove(&index);
                self.render_devices
                    .applied
                    .insert(index, self.render_devices.generation);
            }
            Ok(())
        }

        fn image_import_device_fds(&self) -> std::io::Result<Vec<std::os::fd::OwnedFd>> {
            self.image_import_devices
                .iter()
                .map(|device| rustix::io::fcntl_dupfd_cloexec(device, 0).map_err(Into::into))
                .collect()
        }

        pub fn enable_renderer_workers(&mut self) -> Result<usize, Box<dyn std::error::Error>> {
            let mut enabled = 0usize;
            for index in 0..self.exporters.len() {
                if !self.heads[index].enabled {
                    continue;
                }
                self.enable_head_renderer_worker(index)?;
                if !self.exporters[index].worker_enabled() {
                    return Err("native renderer worker was not established".into());
                }
                enabled = enabled.saturating_add(1);
            }
            Ok(enabled)
        }

        /// How many renderer threads this session runs.
        ///
        /// One per card group when sharing is on, one per enabled head when
        /// it is off. The count is evidence: it is the difference the row
        /// exists to make, and a session claiming to share while running a
        /// thread per head would look identical everywhere else.
        /// What the direct scanout path did this session, summed over heads.
        ///
        /// Summed at report time rather than accumulated per tick because the
        /// exporters own the counts and a head that comes and goes takes its
        /// history with it; a session-level mirror would have to be kept
        /// correct across every topology change to say the same thing.
        /// The output whose head has flipped a client buffer directly, if any.
        ///
        /// Named so a development control can put an overlay on the screen
        /// that is actually scanning one, rather than on whichever output
        /// happens to be first.
        pub fn direct_scanout_output(&self) -> Option<OutputId> {
            std::iter::zip(&self.heads, &self.exporters)
                .find(|(_, exporter)| exporter.direct_scanout_flips() != 0)
                .map(|(head, _)| head.output.id)
        }

        /// What frames cost, both halves and both populations.
        ///
        /// The submit-to-flip half is recorded here, on the head that
        /// flipped; the offer-to-submit half lives in each exporter, which
        /// is the only place that knows how long its own composition pass
        /// took. They are merged rather than kept apart because the question
        /// -- does a direct frame cost less than a composed one -- is about
        /// the whole path, not either end of it.
        pub fn direct_scanout_cost(&self) -> crate::DirectScanoutCost {
            let mut cost = self.cost.clone();
            for exporter in &self.exporters {
                cost.merge(exporter.cost());
            }
            cost
        }

        pub fn direct_scanout_totals(&self) -> LiveProductionDirectScanoutTotals {
            self.exporters.iter().fold(
                LiveProductionDirectScanoutTotals::default(),
                |totals, exporter| LiveProductionDirectScanoutTotals {
                    attempts: totals
                        .attempts
                        .saturating_add(exporter.direct_scanout_attempts()),
                    flips: totals.flips.saturating_add(exporter.direct_scanout_flips()),
                    tests: totals.tests.saturating_add(exporter.direct_scanout_tests()),
                    test_rejections: totals
                        .test_rejections
                        .saturating_add(exporter.direct_scanout_test_rejections()),
                    refusals: totals
                        .refusals
                        .saturating_add(exporter.direct_scanout_refusals()),
                    unsupported: totals
                        .unsupported
                        .saturating_add(exporter.direct_scanout_unsupported()),
                    fallbacks: totals
                        .fallbacks
                        .saturating_add(exporter.direct_scanout_fallbacks()),
                },
            )
        }

        /// How many lowered frames carried each direct-scanout verdict, per
        /// head, indexed as `DirectScanoutVerdict::VERDICTS`.
        ///
        /// Per head rather than summed, because a session's heads answer
        /// differently and the sum hides it: a head with no client contributes
        /// its blank frames to the same column as a head whose client is one
        /// layer short, and reading that total sends someone to the wrong
        /// screen.
        pub fn direct_scanout_head_verdicts(
            &self,
        ) -> Vec<(
            OutputId,
            sophia_engine::RenderHeadId,
            [usize; sophia_engine::DirectScanoutVerdict::COUNT],
        )> {
            self.exporters
                .iter()
                .enumerate()
                .filter_map(|(index, exporter)| {
                    let head = self.heads.get(index)?;
                    Some((
                        head.output.id,
                        head.head,
                        exporter.direct_scanout_verdicts(),
                    ))
                })
                .collect()
        }

        /// The same, summed over heads.
        pub fn direct_scanout_verdicts(
            &self,
        ) -> [usize; sophia_engine::DirectScanoutVerdict::COUNT] {
            self.exporters.iter().fold(
                [0usize; sophia_engine::DirectScanoutVerdict::COUNT],
                |mut totals, exporter| {
                    for (total, count) in
                        std::iter::zip(&mut totals, exporter.direct_scanout_verdicts())
                    {
                        *total = total.saturating_add(count);
                    }
                    totals
                },
            )
        }

        /// Let heads take the direct path, now that startup readiness has
        /// proven a picture reached glass.
        ///
        /// Before that the barrier has no evidence to read: it measures
        /// composed pixels, and a direct frame is never composed. A session
        /// that flipped immediately could put a client on screen and still
        /// time out claiming nothing was presented, which is exactly what one
        /// did.
        pub fn admit_direct_scanout(&mut self) {
            self.direct_scanout_admitted = true;
            if !self.direct_scanout_admissible {
                return;
            }
            for index in 0..self.exporters.len() {
                let mirrored = self.head_indices(self.heads[index].output.id).len() > 1;
                self.exporters[index]
                    .set_direct_scanout_enabled(!mirrored && !self.translation_motion_active);
            }
        }

        pub fn set_translation_motion_active(&mut self, active: bool) {
            if self.translation_motion_active == active {
                return;
            }
            self.translation_motion_active = active;
            self.invalidate_layout_probes();
            for index in 0..self.exporters.len() {
                let mirrored = self.head_indices(self.heads[index].output.id).len() > 1;
                self.exporters[index].set_direct_scanout_enabled(
                    !active
                        && self.direct_scanout_admitted
                        && self.direct_scanout_admissible
                        && !mirrored,
                );
            }
        }

        pub fn renderer_worker_count(&self) -> usize {
            if Self::shared_renderer_worker_enabled() {
                self.groups
                    .iter()
                    .filter(|group| group.renderer_core.is_some())
                    .count()
            } else {
                self.exporters
                    .iter()
                    .enumerate()
                    .filter(|(index, exporter)| {
                        self.heads[*index].enabled && exporter.worker_enabled()
                    })
                    .count()
            }
        }

        pub fn enabled_head_count(&self) -> usize {
            self.heads.iter().filter(|head| head.enabled).count()
        }

        pub fn queue_frame(
            &mut self,
            output: OutputId,
            frame: LiveProductionComposedFrame,
        ) -> LiveProductionCpuFrameQueueStatus {
            let Some(index) = self.primary_head_index(output) else {
                return LiveProductionCpuFrameQueueStatus::NoHead;
            };
            // A group's heads need the frame placed for each of their modes, which
            // the pure-CPU path below cannot express -- it uploads at the frame's
            // own size. An output with one head keeps that path exactly, so no
            // ordinary desktop changes.
            if self.head_indices(output).len() > 1 {
                let indices = self.head_indices(output);
                let statuses = indices
                    .iter()
                    .map(|head_index| {
                        let head = &self.heads[*head_index];
                        reduce_live_production_cpu_frame_queue(
                            head.pending_content,
                            head.submitted_content,
                            head.presented_content,
                            self.exporters[*head_index].worker_in_flight(),
                            head.callback_accepted != 0
                                || head.initial_modeset_submission.is_some(),
                            frame.checksum,
                        )
                    })
                    .collect::<Vec<_>>();
                for unchanged in [
                    LiveProductionCpuFrameQueueStatus::UnchangedPending,
                    LiveProductionCpuFrameQueueStatus::UnchangedSubmitted,
                    LiveProductionCpuFrameQueueStatus::UnchangedPresented,
                ] {
                    if statuses.iter().all(|status| *status == unchanged) {
                        return unchanged;
                    }
                }
                let projected = self.queue_projected_frame(output, &frame);
                return if projected.is_some() {
                    LiveProductionCpuFrameQueueStatus::Queued
                } else {
                    LiveProductionCpuFrameQueueStatus::NoHead
                };
            }
            let status = {
                let head = &self.heads[index];
                reduce_live_production_cpu_frame_queue(
                    head.pending_content,
                    head.submitted_content,
                    head.presented_content,
                    self.exporter(output).is_some_and(
                        crate::NativeGbmRenderedScanoutBufferDiscoveryExporter::worker_in_flight,
                    ),
                    head.callback_accepted != 0 || head.initial_modeset_submission.is_some(),
                    frame.checksum,
                )
            };
            if !matches!(
                status,
                LiveProductionCpuFrameQueueStatus::Queued
                    | LiveProductionCpuFrameQueueStatus::BaselineRequired
            ) {
                return status;
            }
            let frame_id = self.allocate_frame_id();
            let (head, exporter) = self.head_and_exporter(index, output);
            head.pending_nonzero_pixel_bytes = frame.nonzero_pixel_bytes;
            head.last_checksum = frame.checksum;
            head.queue_output_damage_snapshot(frame.output_damage_snapshot.clone());
            head.pending_content = Some(LiveProductionScanoutContent::Cpu {
                frame: frame_id,
                checksum: frame.checksum,
            });
            exporter.set_pending_cpu_frame_with_damage(
                frame.frame,
                frame.checksum,
                frame.output_damage_snapshot,
            );
            status
        }

        pub fn take_presentation_feedback(
            &mut self,
            output: OutputId,
        ) -> Option<LiveProductionNativeFrameRetirement> {
            let (retirement, native) = self.production_page_flips.take_native_retirement(output)?;
            let native = native?;
            let content = native.content;
            Some(LiveProductionNativeFrameRetirement {
                output,
                frame: content.frame(),
                submission: retirement.cycle,
                content,
                direct: native.direct,
                layout_witness: native.layout_witness,
                ust: retirement.retirement.ust,
                msc: retirement.retirement.msc,
            })
        }

        pub fn pending_kernel_page_flip_timestamps(&self) -> usize {
            self.kernel_page_flip_ust.len()
        }

        pub fn discard_presentation_feedback(&mut self, output: Option<OutputId>) {
            self.production_page_flips.discard_retirements(output);
        }

        /// Whether any head of this output has KMS work in flight.
        ///
        /// A mirror group submits into per-head slots, so the runtime's single
        /// per-output submission slot stays empty for one and reports Idle. The
        /// frame service reads that phase to decide whether to poll for
        /// retirement, so a grouped output was never polled, its retirements
        /// never consumed, and Present completions never routed -- a silent
        /// freeze on the first content change rather than an error.
        pub fn output_in_flight(&self, output: OutputId) -> bool {
            self.head_indices(output)
                .into_iter()
                .any(|index| self.heads[index].scanout_submission.is_some())
        }

        /// Whether any head of this output still owes resource cleanup.
        pub fn output_cleanup_pending(&self, output: OutputId) -> bool {
            self.head_indices(output)
                .into_iter()
                .any(|index| self.heads[index].scanout_cleanup.is_some())
                || self.output_topology_cleanup.iter().any(|(head, _)| {
                    self.head_index_for_head(*head)
                        .is_some_and(|index| self.heads[index].output.id == output)
                })
        }

        pub fn pending_frame(&self, output: OutputId) -> bool {
            let mirror = self.head_indices(output).len() > 1;
            self.head_indices(output).into_iter().any(|index| {
                self.exporters[index].pending_frame()
                    || self.heads[index].prepared_scanout.is_some()
                    || self.heads[index].pending_content.is_some()
                    || self.heads[index].rendering_content.is_some()
                    || (!mirror && self.heads[index].scanout_submission.is_some())
            })
        }

        pub fn is_mirror_output(&self, output: OutputId) -> bool {
            self.head_indices(output).len() > 1
        }

        pub fn primary_scanout_in_flight(&self, output: OutputId) -> bool {
            self.primary_head_index(output)
                .is_some_and(|index| self.heads[index].scanout_submission.is_some())
        }

        pub fn primary_cleanup_pending(&self, output: OutputId) -> bool {
            self.primary_head_index(output)
                .is_some_and(|index| self.heads[index].scanout_cleanup.is_some())
        }

        pub fn frame_queue_ready(&self, output: OutputId) -> bool {
            if self.is_mirror_output(output) {
                return !self.mirror_generation_failed(output);
            }
            !self.pending_frame(output)
                && !self.output_in_flight(output)
                && !self.output_cleanup_pending(output)
        }

        pub fn scanout_in_flight(&self, output: OutputId) -> bool {
            self.head_indices(output)
                .into_iter()
                .any(|index| self.heads[index].scanout_submission.is_some())
        }

        pub fn scanout_cleanup_pending(&self, output: OutputId) -> bool {
            self.head_indices(output)
                .into_iter()
                .any(|index| self.heads[index].scanout_cleanup.is_some())
        }

        pub fn mirror_generation_failed(&self, output: OutputId) -> bool {
            self.output_lifecycles
                .get(&output)
                .is_some_and(LiveProductionMirrorGroupLifecycle::failed)
        }

        fn mirror_poison_drained(&self, output: OutputId) -> bool {
            self.mirror_generation_failed(output)
                && !self.scanout_in_flight(output)
                && !self.scanout_cleanup_pending(output)
        }

        pub fn any_head_scanout_in_flight(&self) -> bool {
            self.heads
                .iter()
                .any(|head| head.scanout_submission.is_some())
        }

        /// Heads holding a KMS submission the kernel has not retired.
        ///
        /// Keyed on the submitted sequence rather than on the retained mirror
        /// owner. `scanout_submission` exists only on the mirror path, where a
        /// group parks each head's owner until the cohort joins; an ordinary
        /// extended-desktop head never sets it, so counting it reported zero
        /// for every session that was not mirroring -- including the one this
        /// counter exists to describe. The submitted sequence is set at submit
        /// and taken at retirement on both paths, which is exactly the window
        /// "in flight" names.
        pub fn head_scanout_in_flight_count(&self) -> usize {
            self.heads
                .iter()
                .filter(|head| head.submitted_sequence.is_some())
                .count()
        }

        pub fn any_head_cleanup_pending(&self) -> bool {
            !self.output_topology_cleanup.is_empty()
                || self.layout_probe_cleanup_pending()
                || self.heads.iter().any(|head| head.scanout_cleanup.is_some())
        }

        pub fn submitted_content(&self, output: OutputId) -> Option<LiveProductionScanoutContent> {
            let frame = self.submitted_frame(output)?;
            self.head_indices(output).into_iter().find_map(|index| {
                self.heads[index]
                    .submitted_content
                    .filter(|content| content.frame() == frame)
            })
        }

        /// Returns the logical submitted generation independently of which
        /// physical head still owns `submitted_content`.
        ///
        /// During an asymmetric mirror flip the primary may already have moved
        /// its content to `presented_content` while a sibling remains in flight.
        pub fn submitted_frame(&self, output: OutputId) -> Option<LiveProductionNativeFrameId> {
            if self.head_indices(output).len() > 1 {
                return self
                    .output_lifecycles
                    .get(&output)
                    .and_then(LiveProductionMirrorGroupLifecycle::logically_submitted_frame);
            }
            self.heads[self.primary_head_index(output)?]
                .submitted_content
                .map(LiveProductionScanoutContent::frame)
        }

        /// Returns the immutable scene snapshot retired by the latest accepted
        /// page flip for this output. Pending, rendering, and submitted work is
        /// intentionally invisible here.
        pub fn presented_output_frame(
            &self,
            output: OutputId,
        ) -> Option<&sophia_engine::OutputFrameDamageSnapshot> {
            self.heads[self.primary_head_index(output)?]
                .output_frames
                .presented()
        }

        pub fn presented_frame(&self, output: OutputId) -> Option<LiveProductionNativeFrameId> {
            self.heads[self.primary_head_index(output)?]
                .presented_content
                .map(LiveProductionScanoutContent::frame)
        }

        /// Whether an exact logical frame still has a native owner.
        ///
        /// CPU progress uses this after each production/service turn. A frame
        /// remains live while it is deferred, active in a mirror cohort, or held
        /// by any physical-head queue stage. Once it disappears from all of
        /// those cells without retiring, latest-wins supersession is proven.
        pub fn output_owns_frame(
            &self,
            output: OutputId,
            frame: LiveProductionNativeFrameId,
        ) -> bool {
            if self
                .deferred_mirror_generations
                .get(&output)
                .is_some_and(|generation| generation.frame == frame)
            {
                return true;
            }
            if self
                .output_lifecycles
                .get(&output)
                .is_some_and(|lifecycle| {
                    lifecycle.active_frame() == Some(frame)
                        || lifecycle.generation_is_scanned(frame)
                })
            {
                return true;
            }
            self.head_indices(output).into_iter().any(|index| {
                let head = &self.heads[index];
                [
                    head.pending_content,
                    head.rendering_content,
                    head.submitted_content,
                    head.presented_content,
                ]
                .into_iter()
                .flatten()
                .any(|content| content.frame() == frame)
                    || head.prepared_group_frame == Some(frame)
                    || head.submitted_group_frame == Some(frame)
                    || head.displayed_group_frame == Some(frame)
            })
        }

        pub fn stable_present(&self, output: OutputId, transaction: TransactionId) -> bool {
            self.primary_head_index(output).is_some_and(|index| {
                live_production_scanout_is_stable_present(
                    self.heads[index].presented_content,
                    transaction,
                )
            })
        }

        pub fn presented_mixed_nonzero_rgb_pixels(&self, transaction: TransactionId) -> usize {
            self.outputs()
                .into_iter()
                .filter_map(|output| {
                    let index = self.primary_head_index(output.id)?;
                    match self.heads[index].presented_content {
                        Some(LiveProductionScanoutContent::MixedPresent {
                            transaction: presented,
                            nonzero_rgb_pixels,
                            ..
                        }) if presented == transaction => Some(nonzero_rgb_pixels),
                        _ => None,
                    }
                })
                .max()
                .unwrap_or(0)
        }

        pub fn poll_group_callbacks(
            &mut self,
            group: usize,
        ) -> Result<(), Box<dyn std::error::Error>> {
            let callback_capacity = self
                .heads
                .iter()
                .filter(|head| head.enabled && head.group == group)
                .count()
                .max(1);
            let report = {
                let owner = &mut self.groups[group];
                owner.callbacks.clear();
                owner.timestamps.clear();
                owner.session.collect_native_page_flip_events(
                    &mut owner.callbacks,
                    &mut owner.timestamps,
                    callback_capacity,
                    callback_capacity,
                )
            };
            if report.read_loop.status == crate::LibdrmNativeReadLoopStatus::ReadFailed {
                return Err("native card page-flip read failed".into());
            }
            let mut callbacks = std::mem::take(&mut self.groups[group].callbacks);
            let mut timestamps = std::mem::take(&mut self.groups[group].timestamps);
            let route_result = (|| -> Result<(), Box<dyn std::error::Error>> {
                for timestamp in &mut timestamps {
                    let head = self
                        .heads
                        .iter()
                        .find(|head| {
                            head.group == group && head.enabled && head.head == timestamp.head
                        })
                        .ok_or("native timestamp referenced an inactive or unknown head")?;
                    // The libdrm route was created at discovery time. Dynamic
                    // regrouping changes only the logical output, so normalize
                    // that policy identity through the current opaque head.
                    timestamp.output = head.output.id;
                    self.kernel_page_flip_ust.insert(
                        (timestamp.output, timestamp.head, timestamp.frame_serial),
                        timestamp.ust_usec,
                    );
                }
                for callback in &mut callbacks {
                    // By head, not by output. Two heads of a mirror group share
                    // an output, so only the head identifies the physical owner.
                    let Some(head_index) = self.heads.iter().position(|head| {
                        head.group == group && head.enabled && head.head == callback.head
                    }) else {
                        return Err(format!(
                            "native callback referenced an unknown head: head={} output={}",
                            callback.head.raw(),
                            callback.output.raw(),
                        )
                        .into());
                    };
                    callback.output = self.heads[head_index].output.id;
                    if self.heads[head_index].completion_mode
                        == LiveProductionKmsCompletionMode::OutFenceAuthoritative
                    {
                        self.heads[head_index].late_page_flip_events = self.heads[head_index]
                            .late_page_flip_events
                            .saturating_add(1);
                        self.kernel_page_flip_ust.remove(&(
                            callback.output,
                            callback.head,
                            callback.frame_serial,
                        ));
                        continue;
                    }
                    if self.heads[head_index].pending_callback.is_some() {
                        self.callback_queue_saturated =
                            self.callback_queue_saturated.saturating_add(1);
                        return Err(format!(
                            "native head completion ledger is full: head={} output={}",
                            callback.head.raw(),
                            callback.output.raw(),
                        )
                        .into());
                    }
                    self.heads[head_index].pending_callback = Some(*callback);
                }
                Ok(())
            })();
            callbacks.clear();
            timestamps.clear();
            self.groups[group].callbacks = callbacks;
            self.groups[group].timestamps = timestamps;
            route_result
        }

        /// Poll every DRM card before any output retirement or watchdog check.
        pub fn pump_native_completions(&mut self) -> Result<(), Box<dyn std::error::Error>> {
            for group in 0..self.groups.len() {
                self.poll_group_callbacks(group)?;
            }
            Ok(())
        }
    }

    fn trace_live_native_lifecycle(stage: &str) {
        if std::env::var_os("SOPHIA_LIVE_SESSION_DIAGNOSTIC").is_some() {
            tracing::info!("sophia_live_native_lifecycle schema=1 stage={stage}");
        }
    }
}

#[cfg(all(feature = "libdrm-events", feature = "gbm-probe"))]
pub use persistent_native_scanout::{
    LIVE_PRODUCTION_PAGE_FLIP_HARD_STALL, LiveOutputAllocationContext,
    LiveOutputAllocationFormatPreference, LiveOutputAllocationPreference,
    LivePersistentRenderMetrics, LiveProductionCompletionTimestamp,
    LiveProductionCpuFrameQueueStatus, LiveProductionDirectScanoutTotals,
    LiveProductionHeadCompositionFrame, LiveProductionKmsCompletionSource,
    LiveProductionMirrorGenerationQueue, LiveProductionMirrorGroupBegin,
    LiveProductionMirrorGroupLifecycle, LiveProductionMirrorHeadTransition,
    LiveProductionNativeFrameRetirement, LiveProductionNativeHead, LiveProductionNativeScanout,
    LiveProductionNativeTopologyApplyCoordinator, LiveProductionNativeTopologyApplyPhase,
    LiveProductionNativeTopologyApplyTransition, LiveProductionNativeTopologyCandidateResource,
    LiveProductionNativeTopologyCurrentHead, LiveProductionNativeTopologyDisposition,
    LiveProductionNativeTopologyHeadPlan, LiveProductionNativeTopologyPlan,
    LiveProductionNativeTopologyPlanError, LiveProductionNativeTopologyPreparationPhase,
    LiveProductionNativeTopologyPreparationReport, LiveProductionNativeTopologyResourceCohort,
    LiveProductionNativeTopologyResourceRejection, LiveProductionNativeTopologyResourceTransition,
    LiveProductionPageFlipWatchdogStatus, LiveProductionRendererImageHandoff,
    LiveProductionRetainedFrameQueueRequirement, LiveProductionRetainedSceneQueueStatus,
    LiveProductionRetiredLayoutWitness, LiveProductionScanoutContent,
    LiveProductionSemanticStartupBarrier, LiveRenderDeviceNodeIdentity,
    advance_live_production_renderer_content, finish_live_production_native_initialization,
    live_production_mirror_head_work_frame, live_production_retained_frame_requirement,
    live_production_scanout_is_stable_present, live_topology_frame_renderer_image_requirements,
    plan_live_production_native_topology, project_live_production_published_topology,
    project_mirror_output_damage_snapshot, project_native_cursor_logical_viewport,
    reduce_live_production_completion_timestamp, reduce_live_production_cpu_frame_queue,
    reduce_live_production_head_render_target, reduce_live_production_mirror_generation_queue,
    reduce_live_production_page_flip_watchdog, reduce_live_production_retained_frame_queue,
    reduce_live_production_retained_scene_queue, reduce_live_production_semantic_startup_barrier,
    validate_live_head_composition_frame_batch, validate_live_production_rollback_topology,
    validate_live_production_topology_frames,
};

#[cfg(all(feature = "libdrm-events", feature = "gbm-probe"))]
pub(crate) use persistent_native_scanout::LiveProductionNativeRetirementContent;

#[derive(Debug)]
pub struct LiveNativeMixedDiagnosticComplete {
    pub status: crate::LiveRendererScanoutBufferExportStatus,
    pub detail: crate::LiveRendererScanoutBufferExportDetail,
    pub cpu_layers: usize,
    pub dmabuf_layers: usize,
    pub live_sources: usize,
    pub live_fences: usize,
    pub live_transactions: usize,
}

impl LiveNativeMixedDiagnosticComplete {
    pub fn reduced_log_line(&self, child_outcome: &str) -> String {
        format!(
            "sophia_native_egl_mixed schema=1 case=mixed status={:?} stage={:?} cpu_layers={} dmabuf_layers={} child_outcome={} live_sources={} live_fences={} live_transactions={}",
            self.status,
            self.detail,
            self.cpu_layers,
            self.dmabuf_layers,
            child_outcome,
            self.live_sources,
            self.live_fences,
            self.live_transactions,
        )
    }
}

impl std::fmt::Display for LiveNativeMixedDiagnosticComplete {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.reduced_log_line("completed"))
    }
}

impl std::error::Error for LiveNativeMixedDiagnosticComplete {}
