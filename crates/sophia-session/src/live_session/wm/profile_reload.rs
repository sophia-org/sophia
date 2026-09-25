#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DesktopProfileReloadOutcome {
    Unchanged,
    Declined,
    Applied,
    Deferred,
    RestartRequired,
}

struct DesktopProfileReloadEffects {
    policy_changed: bool,
    output_changed: bool,
    deferred: Vec<sophia_config::DesktopAuthority>,
}

fn authority_values_equal(
    before: &sophia_config::DesktopProfileGeneration,
    after: &sophia_config::DesktopProfileGeneration,
    authority: sophia_config::DesktopAuthority,
) -> bool {
    match (
        before.candidates.get(&authority),
        after.candidates.get(&authority),
    ) {
        (Some(before), Some(after)) => before
            .values
            .iter()
            .map(|value| (&value.key, &value.encoded))
            .eq(after
                .values
                .iter()
                .map(|value| (&value.key, &value.encoded))),
        (None, None) => true,
        _ => false,
    }
}

fn desktop_profile_reload_effects(
    before: &sophia_config::DesktopProfileGeneration,
    after: &sophia_config::DesktopProfileGeneration,
) -> DesktopProfileReloadEffects {
    use sophia_config::DesktopAuthority;
    DesktopProfileReloadEffects {
        policy_changed: !authority_values_equal(before, after, DesktopAuthority::Policy),
        output_changed: !authority_values_equal(before, after, DesktopAuthority::Output),
        deferred: [
            DesktopAuthority::Shell,
            DesktopAuthority::Input,
            DesktopAuthority::Broker,
        ]
        .into_iter()
        .filter(|authority| !authority_values_equal(before, after, *authority))
        .collect(),
    }
}

struct PreparedDesktopLaunch {
    profile: sophia_config::DesktopProfileGeneration,
    launch_profile: sophia_config::DesktopSessionCandidate,
    applications: SessionApplicationConfig,
    shortcuts: sophia_config::DesktopShortcutCandidate,
    commands: SessionCommandRegistry,
    output: Option<sophia_config::DesktopOutputCandidate>,
    core_generation: sophia_config::ConfigGeneration,
}

struct PreviousPolicyProfile {
    fragments: sophia_config::DesktopProfileFragments,
    profile: PreparedAuthorityFragment,
    key: Option<sophia_config::DesktopProfileActivationKey>,
    shortcuts: sophia_config::DesktopProfileCandidateSlot<sophia_config::DesktopShortcutCandidate>,
    launch_spec: ProcessLaunchSpec,
}

struct PendingDesktopReload {
    launch: PreparedDesktopLaunch,
    previous: PreviousPolicyProfile,
    replacement_spec: Option<ProcessLaunchSpec>,
    accepted: Option<(
        sophia_protocol::PolicyConfiguration,
        sophia_engine::WmShortcutRegistry,
    )>,
    deadline: Instant,
}

fn activated_shortcut_slot(
    candidate: sophia_config::DesktopShortcutCandidate,
) -> Result<
    sophia_config::DesktopProfileCandidateSlot<sophia_config::DesktopShortcutCandidate>,
    Box<dyn std::error::Error>,
> {
    let key =
        sophia_config::DesktopProfileActivationKey::new(candidate.generation, candidate.digest);
    let slot = sophia_config::DesktopProfileCandidateSlot::with_candidate(candidate)?;
    Ok(sophia_config::activate_desktop_profile_candidate_slot(
        &slot, key,
    )?)
}

impl PreparedDesktopLaunch {
    fn prepare(
        config: &PersistentXtermSessionConfig,
        prepared: sophia_config::PreparedDesktopProfile,
        generation: u64,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let sophia_config::PreparedDesktopProfile {
            profile,
            candidates,
        } = prepared;
        let effects = desktop_profile_reload_effects(&config.desktop_profile, &profile);
        for authority in effects.deferred {
            crate::session_eprintln!(
                "sophia_live_desktop_profile schema=2 status=reload_deferred authority={} reason=applied_at_session_start",
                authority.name(),
            );
        }
        let active = config
            .session_profile
            .slot()
            .active()
            .or_else(|| config.session_profile.slot().candidate())
            .ok_or("desktop reload has no active Session profile")?;
        let requested = candidates.session;
        let mut session = active.clone();
        session.generation = requested.generation;
        session.digest = requested.digest;
        session.applications.clone_from(&requested.applications);
        session.terminal.clone_from(&requested.terminal);
        session.browser.clone_from(&requested.browser);
        if active.startup != requested.startup
            || active.application_catalog != requested.application_catalog
            || active.logout_enabled != requested.logout_enabled
            || active.control != requested.control
            || active.components != requested.components
        {
            crate::session_eprintln!(
                "sophia_live_desktop_profile schema=2 status=reload_deferred authority=session reason=non_launch_settings"
            );
        }
        let core = config.core_config_state.active();
        let applications = config.session_application_overrides.prepare_live_reload(
            PersistentXtermSessionConfig::applications_from_core(core)?,
            &session,
            &config.applications.startup,
        )?;
        applications.validate_shortcuts(
            &candidates.shortcut,
            // Provider changes require restart; validate against the same
            // resolved capability as startup, not the requested new profile.
            config.shell_shortcuts_enabled,
            false,
        )?;
        let commands = SessionCommandRegistry::prepare(generation, &applications)?;
        Ok(Self {
            profile,
            launch_profile: requested,
            applications,
            shortcuts: candidates.shortcut,
            commands,
            output: effects.output_changed.then_some(candidates.output),
            core_generation: core.generation,
        })
    }
}

impl LiveWmSession {
    fn desktop_reload_pending(&self) -> bool {
        self.desktop_reload.is_some()
    }

    fn publish_desktop_launch(
        &mut self,
        config: &mut PersistentXtermSessionConfig,
        launch: PreparedDesktopLaunch,
        configuration: sophia_protocol::PolicyConfiguration,
        registry: sophia_engine::WmShortcutRegistry,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if config.core_config_state.active().generation != launch.core_generation {
            return Err("desktop launch candidate has a stale core generation".into());
        }
        let slot = activated_shortcut_slot(launch.shortcuts.clone())?;
        // Finish all fallible preparation before publishing any launch state.
        let output = launch.output.map(PreparedOutputProfile::new).transpose()?;
        let public = self
            .public
            .as_mut()
            .ok_or("desktop launch lost policy owner")?;
        if output.as_ref().is_some_and(|profile| {
            configured_output_policy_keys(profile.current()) != public.output_policy_keys
        }) {
            return Err("output policy keys are startup identities; changing them requires a new session".into());
        }
        config.applications = launch.applications;
        config.active_launch_profile = Some(launch.launch_profile);
        config.shortcut_profile_candidate = launch.shortcuts;
        config.dropped_shortcuts.clear();
        public.dropped_default_shortcuts.clear();
        config.desktop_profile = launch.profile;
        if let Some(output) = output {
            config.output_profile = output;
            public.output_topology_reload_pending = true;
        }
        public.shortcut_profile_slot = slot;
        public.actions = configuration.actions.clone();
        public.accepted_configuration = Some(configuration.clone());
        public.configured = true;
        self.command_registry = launch.commands;
        self.shortcuts = Some(WmShortcutRouter::new(registry));
        self.chrome = configuration.chrome;
        self.stage_visual_chrome(self.candidate_chrome_style());
        crate::session_println!(
            "sophia_live_desktop_profile schema=2 status=reload_applied generation={} launch_generation={}",
            config.desktop_profile.generation.raw(),
            self.command_registry.generation,
        );
        Ok(())
    }

    /// Roll back the candidate before another process is started. Active launch
    /// state was never replaced; the old immutable policy files are still held.
    fn rollback_desktop_reload(&mut self) -> Option<ProcessLaunchSpec> {
        let pending = self.desktop_reload.take()?;
        let public = self.public.as_mut()?;
        public._profile_fragments = pending.previous.fragments;
        public._profile_slot = pending.previous.profile;
        public.profile_key = pending.previous.key;
        public.shortcut_profile_slot = pending.previous.shortcuts;
        crate::session_eprintln!(
            "sophia_live_desktop_profile schema=2 status=reload_rolled_back reason=policy_replacement_failed"
        );
        Some(pending.previous.launch_spec)
    }

    fn settle_desktop_reload(
        &mut self,
        config: &mut PersistentXtermSessionConfig,
        input_idle: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self
            .desktop_reload
            .as_ref()
            .is_some_and(|pending| pending.accepted.is_none() && Instant::now() >= pending.deadline)
        {
            if let Some(spec) = self.rollback_desktop_reload() {
                self.pending_policy_launch_spec = Some(spec);
                self.request_deliberate_restart();
            }
            return Ok(());
        }
        if !input_idle {
            return Ok(());
        }
        if let Some((configuration, registry)) = self.pending_policy_configuration.take() {
            let public = self
                .public
                .as_mut()
                .ok_or("accepted policy lost its owner")?;
            if public.connection_epoch == configuration.connection_epoch {
                let session_operation_count = public.session_operations.len();
                public.actions = configuration.actions.clone();
                public.accepted_configuration = Some(configuration.clone());
                public.configured = true;
                self.chrome = configuration.chrome;
                self.shortcuts = Some(WmShortcutRouter::new(registry));
                self.stage_visual_chrome(self.candidate_chrome_style());
                crate::session_println!(
                    "sophia_live_wm_configuration schema=2 status=committed catalog_generation={} session_operation_count={}",
                    configuration.generation,
                    session_operation_count,
                );
            }
        }
        if self
            .desktop_reload
            .as_ref()
            .is_none_or(|pending| pending.accepted.is_none())
        {
            return Ok(());
        }
        if self.desktop_reload.as_ref().is_some_and(|pending| {
            pending.launch.core_generation != config.core_config_state.active().generation
        }) {
            self.pending_policy_launch_spec = self.rollback_desktop_reload();
            self.request_deliberate_restart();
            return Ok(());
        }
        let mut pending = self
            .desktop_reload
            .take()
            .expect("accepted candidate is retained");
        let (configuration, registry) = pending
            .accepted
            .take()
            .expect("accepted configuration is retained");
        self.publish_desktop_launch(config, pending.launch, configuration, registry)?;
        // Shell and broker restarts still consume their startup fragments.
        // Retain that one generation while either authority belongs to this session.
        if self._other_authority_fragments.is_none() {
            self._other_authority_fragments = Some(pending.previous.fragments);
        }
        Ok(())
    }

    pub(crate) fn reload_desktop_profile(
        &mut self,
        config: &mut PersistentXtermSessionConfig,
    ) -> Result<DesktopProfileReloadOutcome, Box<dyn std::error::Error>> {
        if self.desktop_reload_pending()
            || self.force_transport_restart
            || self.control_restart.is_some()
            || self
                .shortcuts
                .as_ref()
                .is_some_and(|router| !router.shortcut_idle())
        {
            return Ok(DesktopProfileReloadOutcome::Deferred);
        }
        let Some(public) = self
            .public
            .as_ref()
            .filter(|public| public.configured && public.profile_key.is_some())
        else {
            return Ok(DesktopProfileReloadOutcome::Deferred);
        };
        let next = config
            .desktop_profile
            .generation
            .raw()
            .checked_add(1)
            .ok_or("desktop generation exhausted")?;
        let loaded = match sophia_config::load_prepared_desktop_profile(
            config.desktop_profile_source.as_deref(),
            sophia_config::ConfigGeneration::from_raw(next),
        ) {
            Ok(loaded) => loaded,
            Err(error) => {
                crate::session_eprintln!(
                    "sophia_live_desktop_profile schema=2 status=reload_declined reason=invalid detail={error}"
                );
                return Ok(DesktopProfileReloadOutcome::Declined);
            }
        };
        if loaded.profile.digest == config.desktop_profile.digest {
            return Ok(DesktopProfileReloadOutcome::Unchanged);
        }
        let policy_changed =
            desktop_profile_reload_effects(&config.desktop_profile, &loaded.profile).policy_changed;
        let mut launch = match PreparedDesktopLaunch::prepare(
            config,
            loaded,
            self.command_registry.generation + 1,
        ) {
            Ok(launch) => launch,
            Err(error) => {
                crate::session_eprintln!(
                    "sophia_live_desktop_profile schema=2 status=reload_declined reason=launch_preparation detail={error}"
                );
                return Ok(DesktopProfileReloadOutcome::Declined);
            }
        };
        launch.commands = launch
            .commands
            .with_policy_launch_roles(!self.command_registry.local_roles);
        if !policy_changed {
            let configuration = public
                .accepted_configuration
                .as_ref()
                .ok_or("configured policy has no accepted configuration")?
                .clone();
            let registry = match resolve_public_shortcuts(
                &launch.shortcuts,
                &configuration,
                configuration.generation,
                &launch.commands,
            ) {
                Ok(registry) => registry,
                Err(reason) => {
                    crate::session_eprintln!(
                        "sophia_live_desktop_profile schema=2 status=reload_declined reason={reason}"
                    );
                    return Ok(DesktopProfileReloadOutcome::Declined);
                }
            };
            self.publish_desktop_launch(config, launch, configuration, registry)?;
            return Ok(DesktopProfileReloadOutcome::Applied);
        }
        let staged = (|| -> Result<_, Box<dyn std::error::Error>> {
            let fragments = sophia_config::restage_desktop_profile(
                &launch.profile,
                &public._profile_fragments,
            )?;
            let key = sophia_config::DesktopProfileActivationKey::from(&launch.profile);
            sophia_config::validate_desktop_profile_fragments(&fragments, key)?;
            let policy = PreparedAuthorityFragment::new(
                &fragments,
                sophia_config::DesktopAuthority::Policy,
                key,
            )?;
            let shortcuts = sophia_config::DesktopProfileCandidateSlot::with_candidate(
                launch.shortcuts.clone(),
            )?;
            let spec = public_policy_launch_spec(
                config,
                self.supervisor
                    .launch_spec()
                    .program
                    .to_str()
                    .ok_or("policy executable is not UTF-8")?,
                &self.socket_path,
                &public.checkpoint_path,
                fragments.path(sophia_config::DesktopAuthority::Policy),
                true,
                self.supervisor
                    .launch_spec()
                    .environment
                    .iter()
                    .find(|(name, _)| name == sophia_runtime::SOPHIA_OUTPUT_SOCKET_ENV)
                    .map(|(_, value)| std::path::Path::new(value)),
            )?;
            Ok((fragments, key, policy, shortcuts, spec))
        })();
        let (fragments, key, policy, shortcuts, spec) = match staged {
            Ok(staged) => staged,
            Err(error) => {
                crate::session_eprintln!(
                    "sophia_live_desktop_profile schema=2 status=reload_declined reason=staging detail={error}"
                );
                return Ok(DesktopProfileReloadOutcome::Declined);
            }
        };
        let public = self
            .public
            .as_mut()
            .expect("configured policy owner is retained");
        let previous = PreviousPolicyProfile {
            fragments: std::mem::replace(&mut public._profile_fragments, fragments),
            profile: std::mem::replace(&mut public._profile_slot, policy),
            key: public.profile_key.replace(key),
            shortcuts: std::mem::replace(&mut public.shortcut_profile_slot, shortcuts),
            launch_spec: self.supervisor.launch_spec().clone(),
        };
        self.desktop_reload = Some(PendingDesktopReload {
            launch,
            previous,
            replacement_spec: Some(spec),
            accepted: None,
            deadline: Instant::now() + Duration::from_secs(5),
        });
        self.request_deliberate_restart();
        Ok(DesktopProfileReloadOutcome::RestartRequired)
    }
}

impl LiveWmSession {
    fn launch_reload_busy(&self) -> bool {
        self.desktop_reload_pending()
            || self.force_transport_restart
            || self.control_restart.is_some()
            || self.public.as_ref().is_none_or(|public| !public.configured)
    }

    fn reload_core_launches(
        &mut self,
        config: &mut PersistentXtermSessionConfig,
        bytes: &[u8],
    ) -> Result<sophia_config::CoreReloadReport, Box<dyn std::error::Error>> {
        if self.launch_reload_busy() {
            return Err("core launch reload overlaps policy preparation".into());
        }
        let prepared = config.prepare_core_config_reload(bytes)?;
        let next = if prepared.report.disposition == sophia_config::ReloadDisposition::Applied {
            let applications = prepared
                .applications
                .as_ref()
                .ok_or("core reload has no applications")?;
            let dropped = applications.validate_shortcuts(
                &config.shortcut_profile_candidate,
                config.shell_shortcuts_enabled,
                config.desktop_profile_source.is_none(),
            )?;
            let commands = SessionCommandRegistry::prepare(
                self.command_registry.generation + 1,
                applications,
            )?
            .with_policy_launch_roles(!self.command_registry.local_roles);
            let configuration = self
                .public
                .as_ref()
                .and_then(|public| public.accepted_configuration.as_ref())
                .ok_or("core launch reload has no accepted policy configuration")?;
            let registry = resolve_public_shortcuts_with_dropped_defaults(
                &config.shortcut_profile_candidate,
                configuration,
                configuration.generation,
                &commands,
                &dropped,
            )?;
            Some((commands, registry, dropped))
        } else {
            None
        };
        let report = config.publish_core_config_reload(prepared);
        if let Some((commands, registry, dropped)) = next {
            self.command_registry = commands;
            self.shortcuts = Some(WmShortcutRouter::new(registry));
            config.dropped_shortcuts.clone_from(&dropped);
            self.public.as_mut().expect("core reload retains public policy").dropped_default_shortcuts = dropped;
        }
        Ok(report)
    }
}

impl LiveWmSession {
    fn stage_policy_configuration(
        &mut self,
        public: &mut LivePublicPolicyState,
        configuration: &sophia_protocol::PolicyConfiguration,
    ) -> Result<sophia_protocol::PolicyProjectionOutcome, Box<dyn std::error::Error>> {
        let admitted_slots = public
            .session_operations
            .iter()
            .map(|operation| operation.slot)
            .collect::<BTreeSet<_>>();
        // The policy client offers its vocabulary before receiving a snapshot
        // of this session's grants. An unavailable standard operation is not
        // an accepted action. Explicit bindings still fail resolution below;
        // unknown slots remain invalid rather than being silently discarded.
        let mut configuration = configuration.clone();
        configuration.actions.retain(|action| {
            action.session_operation_slot.is_none_or(|slot| {
                !(1..=7).contains(&slot) || admitted_slots.contains(&slot)
            })
        });
        let missing_slots = configuration
            .actions
            .iter()
            .filter_map(|action| action.session_operation_slot)
            .filter(|slot| !admitted_slots.contains(slot))
            .collect::<BTreeSet<_>>();
        let slots_valid = missing_slots.is_empty();
        if !slots_valid {
            let missing_slots = missing_slots
                .iter()
                .map(u16::to_string)
                .collect::<Vec<_>>()
                .join(",");
            crate::session_println!(
                "sophia_live_wm_configuration schema=2 status=rejected reason=unavailable_session_slot catalog_generation={} missing_slot_count={} missing_slots={missing_slots}",
                configuration.generation,
                missing_slots.split(',').count(),
            );
        }
        let registry = slots_valid
            .then(|| {
                resolve_public_shortcuts_with_dropped_defaults(
                    public
                        .shortcut_profile_slot
                        .candidate()
                        .expect("public policy retains its prepared shortcut candidate"),
                    &configuration,
                    // Profile activation and the action catalog have separate
                    // generation namespaces. A replacement WM may restart its
                    // catalog at 1 after activating a later desktop profile.
                    configuration.generation,
                    self.desktop_reload
                        .as_ref()
                        .map(|pending| &pending.launch.commands)
                        .unwrap_or(&self.command_registry),
                    if self.desktop_reload.is_some() { &[] } else { &public.dropped_default_shortcuts },
                )
            })
            .and_then(|result| {
                if let Err(reason) = &result {
                    crate::session_eprintln!(
                        "sophia_live_wm_configuration schema=1 status=rejected reason={reason:?}"
                    );
                }
                result.ok()
            });
        let outcome = match registry {
            Some(registry) if configuration.connection_epoch == public.connection_epoch => {
                if let Some(pending) = self.desktop_reload.as_mut() {
                    pending.accepted = Some((configuration.clone(), registry));
                } else {
                    self.pending_policy_configuration = Some((configuration.clone(), registry));
                }
                // Invalidate queued scripts in this same turn, before another cause can dispatch.
                public.control_generation = 0;
                public.control_catalog_serial = public
                    .control_catalog_serial
                    .checked_add(1)
                    .ok_or("control catalog serial exhausted")?;
                public.configured = false;
                sophia_protocol::PolicyProjectionOutcome::Committed
            }
            _ => sophia_protocol::PolicyProjectionOutcome::RejectedInvalid,
        };
        Ok(outcome)
    }
}
