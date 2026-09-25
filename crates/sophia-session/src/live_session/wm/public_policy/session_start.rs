impl LiveWmSession {
    fn from_started_public_config(
        config: &PersistentXtermSessionConfig,
        outputs: &[sophia_engine::HeadlessOutput],
        started_launch: StartedPublicPolicyLaunch,
        output_bootstrap: Option<LiveOutputAuthorityBootstrap>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let StartedPublicPolicyLaunch {
            profile_fragments,
            directory,
            policy_profile,
            shell_profile,
            shortcut_profile_slot,
            broker_profile,
            runtime:
                StartedPublicPolicyRuntime {
                    supervisor,
                    supervisor_state,
                    restart_policy,
                    worker,
                    output_transport,
                    socket_path,
                    checkpoint_path,
                },
            profile_key,
        } = started_launch;

        let (output_service, output_authority, output_capabilities, startup_output_transaction) =
            match (output_transport, output_bootstrap) {
                (
                    Some(transport),
                    Some(LiveOutputAuthorityBootstrap {
                        snapshot,
                        capabilities,
                        startup_candidate,
                    }),
                ) => {
                    let mut authority =
                        crate::live_output_authority::LiveOutputAuthorityOwner::new(
                            1,
                            snapshot.clone(),
                        )?;
                    let startup_transaction = startup_candidate
                        .map(|candidate| -> Result<_, Box<dyn std::error::Error>> {
                            let transaction = TransactionId::from_raw(u64::MAX);
                            let admission = authority.admit(
                                transaction,
                                &sophia_protocol::OutputV1Proposal {
                                    connection_epoch: 1,
                                    candidate,
                                },
                                &capabilities,
                            )?;
                            if !matches!(
                                admission,
                                crate::live_output_authority::LiveOutputAuthorityAdmission::Prepared
                            ) {
                                return Err("startup output candidate did not prepare".into());
                            }
                            tracing::info!(
                                "sophia_live_output_authority schema=3 status=startup_effect_pending transaction={} preserved_topology=true",
                                transaction.raw(),
                            );
                            Ok(transaction)
                        })
                        .transpose()?;
                    let service = sophia_runtime::OutputTransportService::spawn(
                        transport,
                        1,
                        TransactionId::from_raw(1),
                        snapshot,
                    )?;
                    (
                        Some(service),
                        Some(authority),
                        capabilities,
                        startup_transaction,
                    )
                }
                (None, None) => (None, None, Vec::new(), None),
                (Some(_), None) => {
                    return Err("native output role has no capability snapshot".into());
                }
                (None, Some(_)) => {
                    return Err("native output snapshot has no supervised role endpoint".into());
                }
            };

        let (session_operations, operation_actions) = public_session_operations(config);
        let active = outputs
            .first()
            .map(|output| output.id)
            .ok_or("public WM requires at least one output")?;
        let scene = LivePublicPolicyState::initial_scene(outputs, active, session_operations.clone());
        let mut reducer = sophia_engine::PolicyProjectionReducer::new(scene)?;
        reducer.connect(1)?;
        let output_bounds = wm_output_bounds(outputs)
            .into_iter()
            .collect::<BTreeMap<_, _>>();
        // Panels belong to the selected shell. Until a client claims a
        // reservation, policy receives the full output work area.
        let work_areas = output_bounds.clone();
        let output_generations = outputs
            .iter()
            .map(|output| (output.id, 1))
            .collect::<BTreeMap<_, _>>();
        let live_output_ids = outputs
            .iter()
            .map(|output| output.id)
            .collect::<BTreeSet<_>>();
        let mut public = LivePublicPolicyState {
            control_generation: 1,
            control_catalog_serial: 1,
            control_tickets: BTreeMap::new(),
            _profile_fragments: profile_fragments,
            _profile_slot: policy_profile,
            profile_key,
            directory,
            checkpoint_path,
            worker: Some(worker),
            output_service,
            output_authority,
            output_effect_dispatched: false,
            output_topology_reload_pending: false,
            startup_output_transaction,
            output_cancel_requested: None,
            output_pending_connection_epoch: None,
            next_output_snapshot_transaction: 2,
            output_capabilities,
            reducer,
            connection_epoch: 1,
            next_connection_epoch: 2,
            next_transaction: if profile_key.is_some() { 3 } else { 1 },
            configured: false,
            negotiated: false,
            selected_capabilities: 0,
            cycle_submitted: false,
            transport_ready: false,
            queue: VecDeque::with_capacity(WM_OWNER_REQUEST_CAPACITY),
            pending_dirty_outputs: BTreeSet::new(),
            in_flight_source: None,
            in_flight_request: None,
            staged: None,
            presentation_input: Default::default(),
            presentation_capture: Default::default(),
            presentation_receipts: VecDeque::new(),
            presentation_withdrawals: VecDeque::new(),
            presentation_withdrawal_pending: false,
            presentation_scene_dirty: false,
            native_presentation_capable: config.native_scanout,
            prepared: None,
            shortcut_profile_slot,
            actions: Vec::new(),
            dropped_default_shortcuts: config.dropped_shortcuts.clone(),
            accepted_configuration: None,
            launch_classifications: BTreeMap::new(),
            launch_origins: Arc::new(Mutex::new(crate::launch_origin::LaunchOriginRegistry::default())),
            staged_launch_contexts: Vec::new(),
            staged_output_launch_contexts: Vec::new(),
            in_flight_origin_surfaces: Vec::new(),
            outputs: outputs.to_vec(),
            output_bounds,
            output_generations,
            output_policy_keys: configured_output_policy_keys(config.output_profile.current()),
            live_output_ids,
            work_areas,
            session_operations,
            operation_actions,
            expected_operation_slot: None,
            pending_operation: None,
            active_output: active,
            deferred_command: None,
            transport_unavailable: false,
            proof_fault_after: config.wm_public_fault_after,
            proof_fault_triggered: false,
            proof_restart_after_action: config.wm_public_restart_after_action,
            proof_restart_checkpoint_before: None,
            proof_restart_triggered: false,
        };
        public.queue.push_back(LivePublicPolicyCause {
            source: LiveWmProposalSource::Relayout,
            cause: sophia_protocol::PolicyRequestCause::SceneChanged,
            affected_outputs: public.all_outputs(active),
        });
        let session = Self {
            supervisor,
            supervisor_state,
            restart_policy,
            socket_path,
            public: Some(public),
            _shell_profile: Some(shell_profile),
            _broker_profile: Some(broker_profile),
            requests: 0,
            request_peak_depth: 0,
            request_rejections: 0,
            action_requests_ordered: 0,
            stale_responses: 0,
            work_area_relayout_required: false,
            shell_reservation_bands: Vec::new(),
            shortcuts: None,
            command_registry: SessionCommandRegistry::prepare(1, &config.applications)?
                .with_policy_launch_roles(config.launch_surface_proof_requested()),
            desktop_reload: None,
            _other_authority_fragments: None,
            pending_policy_launch_spec: None,
            pending_policy_configuration: None,
            wm_chrome_supported: true,
            chrome: sophia_protocol::WmChromePolicy::default(),
            fallback_chrome: config.surface_chrome_style,
            visual_chrome: config.surface_chrome_style,
            pending_visual_chrome: None,
            force_transport_restart: false,
            committed: 0,
            last_committed_at: None,
            max_request: Duration::ZERO,
            max_queue_dwell: Duration::ZERO,
            restarts: 0,
            degraded: false,
            control_restart: None,
            control_lifetime: None,
        };
        if let Some(pid) = session.supervisor.peer_id() {
            crate::diagnostics::capture_process_identity("wm", pid, 1);
        }
        crate::session_println!(
            "sophia_live_wm schema=4 status=ready adapter=sophia_wm_v1 socket=session_owned epoch=1 restarts=0"
        );
        Ok(session)
    }
}
