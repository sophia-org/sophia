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
                        refresh_millihz: super::refresh::head_refresh_millihz(
                            selection.mode().map(|mode| mode.vrefresh()),
                            record.mode.refresh_millihz,
                        ),
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
                        presented_completion_timestamp: None,
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
                        scanout_custody: crate::PersistentScanoutCustody::default(),
                        displayed_group_frame: None,
                        prepared_scanout: None,
                        prepared_group_frame: None,
                        prepared_worker_was_in_flight: false,
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
                deferred_mirror_generations: composition_queue::DeferredNativeCompositions::default(
                ),
                output_topology_preparation: None,
                output_topology_cleanup: Vec::new(),
                head_table,
                native_frame_owner: crate::NativeFrameOwner::new(),
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
                max_cursor_only_commit: Duration::ZERO,
                cursor_only_commit_total: Duration::ZERO,
                cursor_combined_drops: 0,
                cursor_legacy_fallbacks: 0,
                cursor_path: crate::HardwareCursorPath::LegacyIoctl,
                cursor_initialization_deferrals: 0,
                legacy_cursor_updates_primary_in_flight: 0,
                cursor_update_failures: 0,
                max_cursor_initialization: Duration::ZERO,
                max_cursor_update: Duration::ZERO,
                max_cursor_queue_delay: Duration::ZERO,
            })
        }
}
