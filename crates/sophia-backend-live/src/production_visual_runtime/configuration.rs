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
}
