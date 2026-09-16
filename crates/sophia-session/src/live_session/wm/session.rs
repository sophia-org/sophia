const WM_OWNER_REQUEST_CAPACITY: usize = 16;
const WM_OWNER_REJECTION_DIAGNOSTIC_LIMIT: usize = 16;

const fn report_wm_rejection_diagnostic(rejections: usize) -> bool {
    rejections <= WM_OWNER_REJECTION_DIAGNOSTIC_LIMIT
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LiveWmProposalSource {
    Action(WmActionId),
    Focus(SurfaceId),
    PointerFocus,
    PointerGesture {
        surface: SurfaceId,
        mode: sophia_protocol::WmPointerGestureMode,
    },
    Manage(SurfaceId),
    Relayout,
}

#[cfg(test)]
fn completed_pointer_gesture_geometry(
    gesture: sophia_protocol::WmPointerGestureCompleted,
    initial: Rect,
) -> Rect {
    let delta_x = gesture.end.x.saturating_sub(gesture.start.x);
    let delta_y = gesture.end.y.saturating_sub(gesture.start.y);
    match gesture.mode {
        sophia_protocol::WmPointerGestureMode::Move => Rect {
            x: initial.x.saturating_add(delta_x),
            y: initial.y.saturating_add(delta_y),
            ..initial
        },
        sophia_protocol::WmPointerGestureMode::Resize => Rect {
            width: initial.width.saturating_add(delta_x).max(1),
            height: initial.height.saturating_add(delta_y).max(1),
            ..initial
        },
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LiveWmRequestAdmission {
    Admitted,
    RejectedCapacity,
    Duplicate,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LivePhysicalWmActionDisposition {
    Admitted,
    Coalesced,
    RejectedCapacity,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LiveOrderedWmActionAdmission {
    Admitted,
    RejectedCapacity { report: bool },
}

impl From<LiveWmRequestAdmission> for LivePhysicalWmActionDisposition {
    fn from(admission: LiveWmRequestAdmission) -> Self {
        match admission {
            LiveWmRequestAdmission::Admitted => Self::Admitted,
            LiveWmRequestAdmission::Duplicate => Self::Coalesced,
            LiveWmRequestAdmission::RejectedCapacity => Self::RejectedCapacity,
        }
    }
}

fn require_wm_request_admission(
    admission: LiveWmRequestAdmission,
    source: &'static str,
) -> Result<(), Box<dyn std::error::Error>> {
    match admission {
        LiveWmRequestAdmission::Admitted | LiveWmRequestAdmission::Duplicate => Ok(()),
        LiveWmRequestAdmission::RejectedCapacity => {
            Err(format!("WM {source} request exceeded the owner queue capacity").into())
        }
    }
}

struct LiveWmSession {
    supervisor: ProcessSupervisor,
    supervisor_state: sophia_runtime::SupervisorState,
    restart_policy: RestartPolicy,
    socket_path: std::path::PathBuf,
    public: Option<LivePublicPolicyState>,
    _shell_profile: Option<PreparedAuthorityFragment>,
    _broker_profile: Option<PreparedAuthorityFragment>,
    requests: usize,
    request_peak_depth: usize,
    request_rejections: usize,
    action_requests_ordered: usize,
    stale_responses: usize,
    work_area_relayout_required: bool,
    /// The shell's presented work-area claim, mirrored here because the WM
    /// session owns the reduction. The coordinator that admits it lives with
    /// the shell connection; this is the committed result only.
    shell_reservation_bands: Vec<sophia_protocol::OutputReservation>,
    shortcuts: Option<WmShortcutRouter>,
    command_registry: SessionCommandRegistry,
    desktop_reload: Option<PendingDesktopReload>,
    _other_authority_fragments: Option<sophia_config::DesktopProfileFragments>,
    pending_policy_launch_spec: Option<ProcessLaunchSpec>,
    pending_policy_configuration: Option<(sophia_protocol::PolicyConfiguration, sophia_engine::WmShortcutRegistry)>,
    wm_chrome_supported: bool,
    chrome: sophia_protocol::WmChromePolicy,
    fallback_chrome: sophia_engine::SurfaceChromeStyle,
    visual_chrome: sophia_engine::SurfaceChromeStyle,
    pending_visual_chrome: Option<sophia_engine::SurfaceChromeStyle>,
    force_transport_restart: bool,
    committed: usize,
    last_committed_at: Option<Instant>,
    max_request: Duration,
    max_queue_dwell: Duration,
    restarts: usize,
    degraded: bool,
    control_restart: Option<ControlRestartJob>,
    control_lifetime: Option<ControlProcessLifetime>,
}

struct LiveWmProposal {
    transaction: TransactionId,
    layers: Vec<LayerSnapshot>,
    requested_sizes: BTreeMap<SurfaceId, Size>,
    presentation_states: BTreeMap<SurfaceId, sophia_protocol::PolicyPresentationState>,
    configure_deliveries: usize,
    focus: Option<SurfaceId>,
    timeout: Duration,
    update: WmTransactionUpdate,
    moved_surfaces: usize,
    source: Option<LiveWmProposalSource>,
    policy_settlement: Option<LivePolicySettlementIdentity>,
}

struct LiveWmCommitResult {
    update: WmTransactionUpdate,
    source: Option<LiveWmProposalSource>,
    policy_settlement: Option<LivePolicySettlementIdentity>,
}

struct LiveWmOwnerCommit {
    update: WmTransactionUpdate,
    physical_action: Option<WmActionId>,
    pointer_gesture: Option<sophia_protocol::WmPointerGestureMode>,
    session_action: Option<(TransactionId, WmSessionAction, Option<SurfaceId>)>,
    workspace_projection: Option<LiveWmWorkspaceProjection>,
    clear_focus: Option<(TransactionId, SurfaceId)>,
}

#[derive(Clone, Copy)]
struct LiveWmWorkspaceProjection {
    transaction: TransactionId,
    output: sophia_protocol::OutputId,
    workspace: WorkspaceId,
    visible_surfaces: usize,
    focus_present: bool,
}

impl LiveWmSession {
    fn prepare_public_launch(
        config: &mut PersistentXtermSessionConfig,
    ) -> Result<Option<PreparedPublicPolicyLaunch>, Box<dyn std::error::Error>> {
        if config.wm_process.is_some()
            && config.wm_interface == sophia_config::ExternalWmInterface::SophiaWmV1
        {
            let mut prepared = PreparedPublicPolicyLaunch::new(config)?;
            let key = sophia_config::DesktopProfileActivationKey::from(&config.desktop_profile);
            let report = prepared.prepare_startup(
                &mut config.session_profile,
                &mut config.input_profile,
                &mut config.output_profile,
                &config.desktop_profile_activation,
                key,
            )?;
            if report.disposition
                != crate::desktop_profile_activation::DesktopProfileStartupPreparationDisposition::Prepared
            {
                return Err("desktop profile startup preparation was rejected".into());
            }
            config.desktop_profile_activation = report.model;
            return Ok(Some(prepared));
        }
        Ok(None)
    }

    fn activate_public_launch(
        config: &mut PersistentXtermSessionConfig,
        prepared_launch: Option<PreparedPublicPolicyLaunch>,
    ) -> Result<Option<StartedPublicPolicyLaunch>, Box<dyn std::error::Error>> {
        let Some(mut prepared) = prepared_launch else {
            return Ok(None);
        };
        let key = sophia_config::DesktopProfileActivationKey::from(&config.desktop_profile);
        let activation = prepared.activate_startup_until_policy(
            &mut config.session_profile,
            &mut config.input_profile,
            &mut config.output_profile,
            &config.desktop_profile_activation,
            key,
        )?;
        if activation.disposition
            != crate::desktop_profile_activation::DesktopProfileExternalActivationDisposition::AwaitingPolicy
        {
            config.desktop_profile_activation = activation.model;
            return Err("desktop profile local startup activation was rejected".into());
        }
        let policy_effect = activation
            .effect
            .ok_or("desktop profile startup omitted the external policy effect")?;
        let process = config
            .wm_process
            .clone()
            .ok_or("desktop profile activation requires a public WM process")?;
        let runtime = match prepared.start_runtime(config, &process, Some(key)) {
            Ok(runtime) => runtime,
            Err(error) => {
                config.desktop_profile_activation = prepared.reject_policy_startup(
                    &mut config.session_profile,
                    &mut config.input_profile,
                    &mut config.output_profile,
                    &activation.model,
                    policy_effect,
                )?;
                return Err(error);
            }
        };
        let failure = match runtime.worker.event_timeout(Duration::from_secs(5)) {
            Ok(PolicyTransportEvent::Negotiated) => None,
            Ok(PolicyTransportEvent::Failed(error)) => Some(error),
            Ok(_) => Some("policy worker emitted normal traffic before profile admission".to_owned()),
            Err(RecvTimeoutError::Timeout) => Some("policy profile admission timed out".to_owned()),
            Err(RecvTimeoutError::Disconnected) => {
                Some("policy profile admission worker disconnected".to_owned())
            }
        };
        if let Some(reason) = failure {
            drop(runtime);
            config.desktop_profile_activation = prepared.reject_policy_startup(
                &mut config.session_profile,
                &mut config.input_profile,
                &mut config.output_profile,
                &activation.model,
                policy_effect,
            )?;
            return Err(reason.into());
        }
        let activated_policy_slot =
            sophia_config::activate_desktop_profile_candidate_slot(
                &prepared.policy_profile.slot,
                key,
            );
        let Ok(activated_policy_slot) = activated_policy_slot else {
            drop(runtime);
            config.desktop_profile_activation = prepared.reject_policy_startup(
                &mut config.session_profile,
                &mut config.input_profile,
                &mut config.output_profile,
                &activation.model,
                policy_effect,
            )?;
            return Err("desktop profile policy retention slot rejected activation".into());
        };
        prepared.policy_profile.slot = activated_policy_slot;
        let settled =
            crate::desktop_profile_activation::settle_desktop_profile_policy_activation(
                &activation.model,
                policy_effect,
                true,
            )?;
        if !settled.effects.is_empty() {
            return Err("desktop profile activation emitted effects after promotion".into());
        }
        config.desktop_profile_activation = settled.model;
        Ok(Some(prepared.into_started(runtime, Some(key))))
    }

    fn from_config(
        config: &PersistentXtermSessionConfig,
        outputs: &[sophia_engine::HeadlessOutput],
        public_launch: Option<StartedPublicPolicyLaunch>,
        output_bootstrap: Option<LiveOutputAuthorityBootstrap>,
    ) -> Result<Option<Self>, Box<dyn std::error::Error>> {
        let Some(_) = config.wm_process.as_deref() else {
            if public_launch.is_some() {
                return Err("public WM preparation exists without a configured process".into());
            }
            return Ok(None);
        };
        let started =
            public_launch.ok_or("public WM launch requires an activated desktop profile")?;
        Self::from_started_public_config(config, outputs, started, output_bootstrap).map(Some)
    }

    /// Replaces the policy client because someone asked, not because
    /// something broke.
    ///
    /// The restart machinery is the same one a transport fault uses: the
    /// process is replaced, the profile handshake runs again, and the
    /// checkpoint carries the windows across. Only the evidence differs, and
    /// it has to, because a restart nobody requested is a fault and a restart
    /// someone pressed a key for is not.
    pub(crate) fn request_deliberate_restart(&mut self) {
        self.request_transport_restart("wm_restart_requested", None);
    }

    fn request_transport_restart(&mut self, reason: &str, error: Option<&str>) {
        self.force_transport_restart = true;
        crate::session_println!(
            "sophia_live_wm schema=2 status=restart_requested reason={reason} error={}",
            error.unwrap_or("none"),
        );
    }

    fn poll_restart(
        &mut self,
        layout: &mut PersistentLiveLayout,
        output: sophia_engine::HeadlessOutput,
    ) -> Result<Option<LiveWmProposal>, Box<dyn std::error::Error>> {
        self.poll_public_restart(layout, output)
    }

    fn enqueue_manage(
        &mut self,
        surface: SurfaceId,
        layout: &PersistentLiveLayout,
        output: sophia_engine::HeadlessOutput,
    ) -> Result<LiveWmRequestAdmission, Box<dyn std::error::Error>> {
        if !layout.is_policy_managed(surface) {
            return Ok(LiveWmRequestAdmission::Duplicate);
        }
        let public = self.public.as_mut().ok_or("public WM state is unavailable")?;
        Ok(public.queue_cause(LivePublicPolicyCause {
            source: LiveWmProposalSource::Manage(surface),
            cause: sophia_protocol::PolicyRequestCause::SceneChanged,
            affected_outputs: public.all_outputs(output.id),
        }))
    }

    fn enqueue_relayout(
        &mut self,
        layout: &PersistentLiveLayout,
        output: sophia_engine::HeadlessOutput,
    ) -> Result<LiveWmRequestAdmission, Box<dyn std::error::Error>> {
        let _ = layout;
        let public = self.public.as_mut().ok_or("public WM state is unavailable")?;
        Ok(public.queue_cause(LivePublicPolicyCause {
            source: LiveWmProposalSource::Relayout,
            cause: sophia_protocol::PolicyRequestCause::SceneChanged,
            affected_outputs: public.all_outputs(output.id),
        }))
    }

    fn enqueue_surface_removed(
        &mut self,
        surface: SurfaceId,
    ) -> Result<LiveWmRequestAdmission, Box<dyn std::error::Error>> {
        let public = self.public.as_mut().ok_or("public WM state is unavailable")?;
        public.launch_classifications.remove(&surface);
        if let Ok(mut origins) = public.launch_origins.lock() { origins.withdraw(surface); }
        let active = public
            .outputs
            .first()
            .map(|output| output.id)
            .ok_or("public WM has no live output")?;
        Ok(public.queue_cause(LivePublicPolicyCause {
            // Relayout rather than Manage, so closing an application asks for
            // one relayout instead of one per surface it owned.
            //
            // The queue deduplicates by source, and Manage names a surface, so
            // withdrawals never coalesced with each other: quitting a browser
            // with seventeen surfaces filled a sixteen-deep queue and ended
            // the session. Nothing downstream wanted the identity here --
            // Manage carries it so a committed proposal can claim a launch
            // classification, which is about a surface arriving, and the
            // per-surface bookkeeping for a departure already happened on the
            // line above. The policy interface is snapshot-based, so one
            // relayout after any number of removals still tells the whole
            // truth.
            source: LiveWmProposalSource::Relayout,
            cause: sophia_protocol::PolicyRequestCause::SceneChanged,
            affected_outputs: public.all_outputs(active),
        }))
    }

    fn register_launch_placement(
        &mut self,
        surface: SurfaceId,
        classification: u64,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if !surface.is_valid() || classification == 0 {
            return Err("trusted launch placement has an invalid identity".into());
        }
        let public = self.public.as_mut().ok_or("public WM state is unavailable")?;
        match public.launch_classifications.entry(surface) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(classification);
                Ok(())
            }
            std::collections::btree_map::Entry::Occupied(entry)
                if *entry.get() == classification =>
            {
                Ok(())
            }
            std::collections::btree_map::Entry::Occupied(_) => {
                Err("trusted launch placement changed for one surface".into())
            }
        }
    }

    /// Queue the action behind a view pill the shell was actually shown.
    ///
    /// The action must appear in the current publication for that output. A
    /// shell echoes identities it was given; it cannot mint one, so an action
    /// it never received -- or one that belonged to a different output -- buys
    /// nothing. Drawing a pill is not authority to invoke arbitrary policy.
    fn enqueue_indicator_action(
        &mut self,
        action: WmActionId,
        output: sophia_protocol::OutputId,
    ) -> Result<LiveIndicatorAdmissionResult, Box<dyn std::error::Error>> {
        let public = self.public.as_mut().ok_or("public WM state is unavailable")?;
        LiveIndicatorAdmission {
            policy_connection_epoch: public.connection_epoch,
            publication: &public.reducer.indicator_publication(),
            outputs: &public.outputs,
            output_generations: &public.output_generations,
            capabilities: public.selected_capabilities,
            next_transaction: &mut public.next_transaction,
            queue: &mut public.queue,
            in_flight_source: public.in_flight_source,
            in_flight: public.in_flight_request.is_some(),
        }.enqueue(action, output)
    }

    fn enqueue_action(
        &mut self,
        action: WmActionId,
        _layout: &PersistentLiveLayout,
        _output: sophia_engine::HeadlessOutput,
    ) -> Result<LiveOrderedWmActionAdmission, Box<dyn std::error::Error>> {
        let public = self.public.as_mut().ok_or("public WM state is unavailable")?;
        let activation_serial = public.mint_transaction()?.raw();
        let active_output = public.active_output;
        let admission = public.queue_cause(LivePublicPolicyCause {
            source: LiveWmProposalSource::Action(action),
            cause: sophia_protocol::PolicyRequestCause::Action {
                activation_serial,
                action,
            },
            affected_outputs: public.all_outputs(active_output),
        });
        match admission {
            LiveWmRequestAdmission::Admitted => {
                self.action_requests_ordered = self.action_requests_ordered.saturating_add(1);
                Ok(LiveOrderedWmActionAdmission::Admitted)
            }
            LiveWmRequestAdmission::RejectedCapacity => {
                self.request_rejections = self.request_rejections.saturating_add(1);
                Ok(LiveOrderedWmActionAdmission::RejectedCapacity {
                    report: report_wm_rejection_diagnostic(self.request_rejections),
                })
            }
            LiveWmRequestAdmission::Duplicate => {
                unreachable!("ordered WM actions are never duplicate-elided")
            }
        }
    }

    fn enqueue_pointer_interaction(
        &mut self,
        interaction: FloatingPointerPolicyInteraction,
        layout: &PersistentLiveLayout,
    ) -> Result<LiveWmRequestAdmission, Box<dyn std::error::Error>> {
        if !layout.is_policy_managed(interaction.surface) {
            return Ok(LiveWmRequestAdmission::Duplicate);
        }
        let source = LiveWmProposalSource::PointerGesture {
            surface: interaction.surface,
            mode: interaction.mode,
        };
        let Some(public) = self.public.as_mut() else {
            return Ok(LiveWmRequestAdmission::Duplicate);
        };
        let outline = clamp_floating_pointer_outline(
            FloatingPointerOutline {
                surface: interaction.surface,
                start: interaction.start,
                geometry: interaction.geometry,
            },
            &wm_output_bounds(&public.outputs),
        )
        .ok_or("pointer interaction started outside every public-policy output")?;
        let output = public
            .reducer
            .committed()
            .into_iter()
            .find(|projection| {
                projection
                    .placements
                    .iter()
                    .any(|placement| placement.surface == interaction.surface)
            })
            .map(|projection| projection.output)
            .ok_or("pointer interaction target is absent from public-policy state")?;
        let affected_outputs = if output == public.active_output {
            vec![output]
        } else {
            vec![public.active_output, output]
        };
        let cause = LivePublicPolicyCause {
            source,
            cause: sophia_protocol::PolicyRequestCause::Interaction {
                phase: interaction.phase,
                kind: match interaction.mode {
                    sophia_protocol::WmPointerGestureMode::Move => {
                        sophia_protocol::PolicyInteractionKind::Move
                    }
                    sophia_protocol::WmPointerGestureMode::Resize => {
                        sophia_protocol::PolicyInteractionKind::Resize
                    }
                },
                axis: sophia_protocol::PolicyInteractionAxis::None,
                target: interaction.surface,
                geometry: outline.geometry,
            },
            affected_outputs,
        };
        Ok(if interaction.phase == sophia_protocol::PolicyInteractionPhase::Cancel {
            public.queue_security_cancel(cause)
        } else {
            public.queue_cause(cause)
        })
    }

    fn enqueue_pointer_gesture(
        &mut self,
        gesture: sophia_protocol::WmPointerGestureCompleted,
        layout: &PersistentLiveLayout,
    ) -> Result<LiveWmRequestAdmission, Box<dyn std::error::Error>> {
        if !layout.is_policy_managed(gesture.surface) {
            return Ok(LiveWmRequestAdmission::Duplicate);
        }
        // Public policy receives Begin/Update/End above. The completed gesture
        // is an Engine-side compatibility notification and carries no second
        // policy request.
        Ok(LiveWmRequestAdmission::Duplicate)
    }

    /// Called only after an exact presented tab action and its broker grant
    /// have been validated. Hidden members need no current renderable layer.
    fn enqueue_tab_focus(&mut self, surface:SurfaceId, output:sophia_protocol::OutputId)
        -> Result<LiveWmRequestAdmission,Box<dyn std::error::Error>> {
        let public=self.public.as_mut().ok_or("public WM state is unavailable")?;
        let eligible=public.reducer.indicator_publication().tab_groups.iter().any(|g|g.output==output && g.members.contains(&surface))
            && public.reducer.scene().surfaces.iter().any(|s|s.surface==surface && s.capabilities.focusable && !s.current_state.minimized);
        if !eligible {return Ok(LiveWmRequestAdmission::Duplicate);}
        let affected_outputs=if output==public.active_output{vec![output]}else{vec![public.active_output,output]};
        Ok(public.queue_cause(LivePublicPolicyCause{source:LiveWmProposalSource::Focus(surface),
            cause:sophia_protocol::PolicyRequestCause::Focus{target:surface},affected_outputs}))
    }

    fn enqueue_focus(
        &mut self,
        surface: SurfaceId,
        layout: &PersistentLiveLayout,
        output: sophia_engine::HeadlessOutput,
    ) -> Result<LiveWmRequestAdmission, Box<dyn std::error::Error>> {
        if !layout.layers.contains_key(&surface) {
            return Err("pointer focus target is missing from the live layout".into());
        }
        let public = self.public.as_mut().ok_or("public WM state is unavailable")?;
        let target_output = public
            .reducer
            .committed()
            .into_iter()
            .find(|projection| {
                projection
                    .placements
                    .iter()
                    .any(|placement| placement.surface == surface)
            })
            .map_or(output.id, |projection| projection.output);
        let affected_outputs = if target_output == public.active_output {
            vec![target_output]
        } else {
            vec![public.active_output, target_output]
        };
        Ok(public.queue_cause(LivePublicPolicyCause {
            source: LiveWmProposalSource::Focus(surface),
            cause: sophia_protocol::PolicyRequestCause::Focus { target: surface },
            affected_outputs,
        }))
    }

    fn poll_request(
        &mut self,
        layout: &mut PersistentLiveLayout,
        output: sophia_engine::HeadlessOutput,
        allow_new_cycle: bool,
    ) -> Result<Option<LiveWmProposal>, Box<dyn std::error::Error>> {
        // Ahead of the public branch, not behind it. A work area that has moved
        // invalidates the geometry every policy computed from it, and which kind
        // of policy that was is not part of the question. `enqueue_relayout`
        // opens by handling the public case, so it was written for this path;
        // the check that reaches it simply sat below the early return and never
        // did. Chrome clearance changing from zero to two raised this flag, three
        // writers set it and one reader read it, and that reader was unreachable
        // whenever a public policy was driving -- which is every session that
        // runs one. Windows stayed placed against the old clearance while their
        // focus ring was drawn against the new one, so the ring landed outside
        // the output it belonged to and only the sliver that crossed into a
        // neighbouring output was ever visible.
        if self.work_area_relayout_required {
            match self.enqueue_relayout(layout, output)? {
                LiveWmRequestAdmission::Admitted | LiveWmRequestAdmission::Duplicate => {}
                LiveWmRequestAdmission::RejectedCapacity => {
                    return Err("WM work-area relayout exceeded the owner request capacity".into());
                }
            }
        }
        self.poll_public_request(layout, output, allow_new_cycle)
    }

    fn mark_committed(&mut self) {
        self.committed = self.committed.saturating_add(1);
        self.last_committed_at = Some(Instant::now());
    }
}

impl LiveWmSession {
    fn pointer_focus_enabled(&self) -> bool {
        self.public.as_ref().is_some_and(|p| p.configured && p.negotiated &&
            p.selected_capabilities & sophia_protocol::SOPHIA_WM_CAPABILITY_POINTER_FOCUS != 0)
    }

    fn pointer_focus_pending(&self) -> bool {
        self.public.as_ref().is_some_and(|p|
            p.in_flight_source == Some(LiveWmProposalSource::PointerFocus) ||
            p.queue.iter().any(|c| c.source == LiveWmProposalSource::PointerFocus))
    }

    fn enqueue_pointer_focus(&mut self, observation: PresentedPointerFocus) -> LiveWmRequestAdmission {
        if !self.pointer_focus_enabled() { return LiveWmRequestAdmission::Duplicate; }
        let public = self.public.as_mut().expect("negotiated policy");
        if observation.output == public.active_output && observation.target.is_none_or(|target|
            public.reducer.scene().outputs.iter().any(|output|
                output.output == observation.output && output.focus == Some(target))) {
            return LiveWmRequestAdmission::Duplicate;
        }
        let cause = sophia_protocol::PolicyRequestCause::PointerFocus {
            output: observation.output, target: observation.target,
        };
        if !policy_cause_subject_is_live(cause, public.reducer.scene()) {
            return LiveWmRequestAdmission::Duplicate;
        }
        public.queue_cause(LivePublicPolicyCause {
            source: LiveWmProposalSource::PointerFocus,
            cause,
            affected_outputs: public.all_outputs(observation.output),
        })
    }
}
