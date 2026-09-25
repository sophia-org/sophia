#[derive(Clone, Debug)]
struct LivePublicPolicyCause {
    source: LiveWmProposalSource,
    cause: sophia_protocol::PolicyRequestCause,
    affected_outputs: Vec<sophia_protocol::OutputId>,
}

/// Whether the surface a cause was raised about is still in the scene.
///
/// The projection reducer validates this itself and rejects a cause naming a
/// withdrawn surface, but its rejection is an error that ends the session
/// rather than a signal to skip. Checking here keeps a queued cause from
/// outliving its own subject.
fn policy_cause_subject_is_live(
    cause: sophia_protocol::PolicyRequestCause,
    scene: &sophia_protocol::PolicySceneSnapshot,
) -> bool {
    let live = |target| scene.surfaces.iter().any(|surface| surface.surface == target);
    match cause {
        sophia_protocol::PolicyRequestCause::OutputAction { output, output_generation, .. } => {
            scene.outputs.iter().any(|o| o.output == output && o.generation == output_generation)
        }
        sophia_protocol::PolicyRequestCause::PointerFocus { output, target } => {
            scene.outputs.iter().any(|o| o.output == output) && target.is_none_or(|t|
                scene.surfaces.iter().any(|s| s.surface == t && s.current_output == Some(output) && s.capabilities.focusable))
        }
        sophia_protocol::PolicyRequestCause::Focus { target }
        | sophia_protocol::PolicyRequestCause::Interaction { target, .. } => live(target),
        _ => true,
    }
}

/// Narrows a queued cause's outputs to those the scene still has.
///
/// A cause names the outputs it was raised for, and it may have been queued
/// before a topology change replaced them. Those outputs are a hint about where
/// work is owed rather than an identity, so they are resolved against the scene
/// the request will actually carry. Every live output is returned when nothing
/// it named survived: a cause that outlived its outputs still needs servicing,
/// because the topology moving is itself a reason to lay out again, and the
/// alternative is refusing a request whose only fault is that it waited.
fn resolve_public_policy_affected_outputs(
    affected: Vec<sophia_protocol::OutputId>,
    live: impl IntoIterator<Item = sophia_protocol::OutputId>,
) -> Vec<sophia_protocol::OutputId> {
    let live = live.into_iter().collect::<std::collections::BTreeSet<_>>();
    let retained = affected
        .into_iter()
        .filter(|output| live.contains(output))
        .collect::<Vec<_>>();
    if retained.is_empty() {
        let mut all = live.into_iter().collect::<Vec<_>>();
        all.sort_by_key(|output| output.raw());
        return all;
    }
    retained
}

fn public_launch_classification_snapshot(
    classifications: &BTreeMap<SurfaceId, u64>,
    scene: &sophia_protocol::PolicySceneSnapshot,
) -> Vec<sophia_protocol::PolicySurfaceClassification> {
    let live_surfaces = scene
        .surfaces
        .iter()
        .map(|surface| surface.surface)
        .collect::<BTreeSet<_>>();
    classifications
        .iter()
        .filter(|(surface, _)| live_surfaces.contains(surface))
        .map(|(surface, classification)| sophia_protocol::PolicySurfaceClassification {
            surface: *surface,
            classification: *classification,
        })
        .collect()
}

/// Retains focus only when the same complete snapshot proves it is usable.
///
/// Committed policy may still name a surface during the owner turn that
/// withdraws it. The snapshot must not carry that stale identity after its
/// surface record has disappeared: independent clients validate the complete
/// transfer before they reconcile private policy state.
fn public_policy_snapshot_focus(
    output: sophia_protocol::OutputId,
    focus: Option<SurfaceId>,
    surfaces: &[sophia_protocol::PolicySurfaceSnapshot],
) -> Option<SurfaceId> {
    focus.filter(|focus| {
        surfaces.iter().any(|surface| {
            surface.surface == *focus
                && surface.current_output == Some(output)
                && surface.capabilities.focusable
                && !surface.current_state.minimized
        })
    })
}

fn consume_public_launch_classification(
    classifications: &mut BTreeMap<SurfaceId, u64>,
    source: Option<LiveWmProposalSource>,
    outcome: sophia_protocol::PolicyProjectionOutcome,
) -> Option<(SurfaceId, u64)> {
    if outcome != sophia_protocol::PolicyProjectionOutcome::Committed {
        return None;
    }
    let Some(LiveWmProposalSource::Manage(surface)) = source else {
        return None;
    };
    classifications
        .remove(&surface)
        .map(|classification| (surface, classification))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct LivePolicySettlementIdentity {
    connection_epoch: u64,
    request_id: u64,
    scene_generation: u64,
    transaction: TransactionId,
    expect_session_operation: bool,
    session_operation: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PublicPolicyFaultPoint {
    ProposalStaged,
    FrontendPending,
    Prepared,
    TerminalOutcomeQueued,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PolicyCheckpointIdentity {
    device: u64,
    inode: u64,
}

fn policy_checkpoint_identity(
    path: &std::path::Path,
) -> Result<Option<PolicyCheckpointIdentity>, std::io::Error> {
    match std::fs::metadata(path) {
        Ok(metadata) => Ok(Some(PolicyCheckpointIdentity {
            device: metadata.dev(),
            inode: metadata.ino(),
        })),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn policy_checkpoint_replaced(
    before: Option<PolicyCheckpointIdentity>,
    current: Option<PolicyCheckpointIdentity>,
) -> bool {
    current.is_some() && before != current
}

impl PublicPolicyFaultPoint {
    fn parse(value: &str) -> Result<Self, Box<dyn std::error::Error>> {
        match value {
            "proposal_staged" => Ok(Self::ProposalStaged),
            "frontend_pending" => Ok(Self::FrontendPending),
            "prepared" => Ok(Self::Prepared),
            "terminal_outcome_queued" => Ok(Self::TerminalOutcomeQueued),
            _ => Err(format!(
                "--wm-proof-fault-after expects proposal_staged, frontend_pending, prepared, or terminal_outcome_queued; got {value:?}"
            )
            .into()),
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::ProposalStaged => "proposal_staged",
            Self::FrontendPending => "frontend_pending",
            Self::Prepared => "prepared",
            Self::TerminalOutcomeQueued => "terminal_outcome_queued",
        }
    }
}

struct LivePublicPolicyState {
    control_generation: u64,
    control_catalog_serial: u64,
    control_tickets: BTreeMap<u64, sophia_runtime::ControlTicket>,
    worker: Option<PolicyTransportWorker>,
    output_service: Option<sophia_runtime::OutputTransportService>,
    output_authority: Option<crate::live_output_authority::LiveOutputAuthorityOwner>,
    output_effect_dispatched: bool,
    /// A reloaded profile asked for a different output topology, and the owner
    /// loop has not yet built a candidate from it.
    ///
    /// The request is a flag rather than the candidate itself because building
    /// one needs the native scanout, which the policy state does not hold; the
    /// owner loop has it and does the work when the session is next idle.
    output_topology_reload_pending: bool,
    /// Private desktop-profile transaction. It shares the physical authority
    /// reducer with client proposals but has no protocol peer awaiting an outcome.
    startup_output_transaction: Option<TransactionId>,
    output_cancel_requested: Option<(TransactionId, String)>,
    output_pending_connection_epoch: Option<u64>,
    next_output_snapshot_transaction: u64,
    output_capabilities: Vec<sophia_backend_live::LibdrmNativeOutputCapability>,
    _profile_fragments: sophia_config::DesktopProfileFragments,
    _profile_slot: PreparedAuthorityFragment,
    profile_key: Option<sophia_config::DesktopProfileActivationKey>,
    checkpoint_path: std::path::PathBuf,
    directory: PolicySessionDirectory,
    reducer: sophia_engine::PolicyProjectionReducer,
    connection_epoch: u64,
    next_connection_epoch: u64,
    next_transaction: u64,
    configured: bool,
    negotiated: bool,
    selected_capabilities: u64,
    cycle_submitted: bool,
    transport_ready: bool,
    queue: VecDeque<LivePublicPolicyCause>,
    pending_dirty_outputs: BTreeSet<sophia_protocol::OutputId>,
    in_flight_source: Option<LiveWmProposalSource>,
    in_flight_request: Option<sophia_protocol::PolicyProjectionRequest>,
    staged: Option<sophia_engine::StagedPolicyProjection>,
    prepared: Option<LivePolicySettlementIdentity>,
    shortcut_profile_slot:
        sophia_config::DesktopProfileCandidateSlot<sophia_config::DesktopShortcutCandidate>,
    actions: Vec<sophia_protocol::PolicyActionRegistration>,
    accepted_configuration: Option<sophia_protocol::PolicyConfiguration>,
    /// One-shot trusted launch classes retained until the surface's manage
    /// projection commits. Reconnects replay them; rejected/stale cycles do not
    /// consume them.
    launch_classifications: BTreeMap<SurfaceId, u64>,
    launch_origins: Arc<Mutex<crate::launch_origin::LaunchOriginRegistry>>,
    staged_launch_contexts: Vec<sophia_protocol::PolicyLaunchContext>,
    staged_output_launch_contexts: Vec<sophia_protocol::PolicyOutputLaunchContext>,
    in_flight_origin_surfaces: Vec<SurfaceId>,
    outputs: Vec<sophia_engine::HeadlessOutput>,
    output_bounds: BTreeMap<sophia_protocol::OutputId, Rect>,
    output_generations: BTreeMap<sophia_protocol::OutputId, u64>,
    output_policy_keys: BTreeMap<String, u64>,
    live_output_ids: BTreeSet<sophia_protocol::OutputId>,
    work_areas: BTreeMap<sophia_protocol::OutputId, Rect>,
    session_operations: Vec<sophia_protocol::PolicySessionOperation>,
    dropped_default_shortcuts: Vec<sophia_config::DesktopSessionShortcut>,
    operation_actions: BTreeMap<u64, WmSessionAction>,
    expected_operation_slot: Option<u16>,
    pending_operation: Option<(TransactionId, sophia_protocol::PolicySessionOperationRequest)>,
    active_output: sophia_protocol::OutputId,
    deferred_command: Option<PolicyTransportCommand>,
    transport_unavailable: bool,
    proof_fault_after: Option<PublicPolicyFaultPoint>,
    proof_fault_triggered: bool,
    proof_restart_after_action: Option<WmActionId>,
    proof_restart_checkpoint_before: Option<Option<PolicyCheckpointIdentity>>,
    proof_restart_triggered: bool,
}

struct PreparedPublicPolicyLaunch {
    profile_fragments: sophia_config::DesktopProfileFragments,
    directory: PolicySessionDirectory,
    policy_profile: PreparedAuthorityFragment,
    shell_profile: PreparedAuthorityFragment,
    shortcut_profile_slot:
        sophia_config::DesktopProfileCandidateSlot<sophia_config::DesktopShortcutCandidate>,
    broker_profile: PreparedAuthorityFragment,
}

struct StartedPublicPolicyLaunch {
    runtime: StartedPublicPolicyRuntime,
    profile_fragments: sophia_config::DesktopProfileFragments,
    policy_profile: PreparedAuthorityFragment,
    shell_profile: PreparedAuthorityFragment,
    shortcut_profile_slot:
        sophia_config::DesktopProfileCandidateSlot<sophia_config::DesktopShortcutCandidate>,
    broker_profile: PreparedAuthorityFragment,
    profile_key: Option<sophia_config::DesktopProfileActivationKey>,
    directory: PolicySessionDirectory,
}

struct StartedPublicPolicyRuntime {
    supervisor: ProcessSupervisor,
    supervisor_state: sophia_runtime::SupervisorState,
    restart_policy: RestartPolicy,
    worker: PolicyTransportWorker,
    output_transport: Option<sophia_runtime::OutputSessionTransport>,
    socket_path: std::path::PathBuf,
    checkpoint_path: std::path::PathBuf,
}

/// Whether rejecting a response with this outcome leaves the owner owing the
/// client a replacement cycle.
///
/// This is the owner half of the reference client's
/// `stateless_reference_projection_decision`. The two must agree: a client that
/// retries by waiting for a fresh snapshot dies behind its socket deadline if
/// the owner considers itself idle, and the owner is the party that observed
/// the scene move.
///
/// An invalid rejection deliberately does not re-arm. The scene did not move,
/// so re-offering the cycle would spin on the same faulty proposal; ending the
/// connection and letting the supervisor replace the client is the fail-closed
/// answer. A disconnected client has no cycle to receive.
const fn public_policy_rearm_after_outcome(
    outcome: sophia_protocol::PolicyProjectionOutcome,
) -> bool {
    match outcome {
        sophia_protocol::PolicyProjectionOutcome::RejectedStale
        | sophia_protocol::PolicyProjectionOutcome::TimedOut => true,
        sophia_protocol::PolicyProjectionOutcome::Committed
        | sophia_protocol::PolicyProjectionOutcome::RejectedInvalid
        | sophia_protocol::PolicyProjectionOutcome::Disconnected => false,
    }
}

/// Folds owner-observed dirty outputs into at most one queued relayout cause.
///
/// Merging keeps the queue bounded: a stale-rejection storm re-arms repeatedly
/// but can never enqueue more than the one relayout entry it finds or creates.
fn materialize_public_dirty_cause(
    queue: &mut VecDeque<LivePublicPolicyCause>,
    pending: &mut BTreeSet<sophia_protocol::OutputId>,
    in_flight_source: Option<LiveWmProposalSource>,
) {
    if pending.is_empty() || in_flight_source == Some(LiveWmProposalSource::Relayout) {
        return;
    }
    if let Some(queued) = queue
        .iter_mut()
        .find(|queued| queued.source == LiveWmProposalSource::Relayout)
    {
        let mut outputs = queued
            .affected_outputs
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        outputs.append(pending);
        queued.affected_outputs = outputs.into_iter().collect();
        return;
    }
    let affected_outputs = std::mem::take(pending).into_iter().collect();
    queue.push_back(LivePublicPolicyCause {
        source: LiveWmProposalSource::Relayout,
        cause: sophia_protocol::PolicyRequestCause::SceneChanged,
        affected_outputs,
    });
}

fn enqueue_public_policy_cause(
    queue: &mut VecDeque<LivePublicPolicyCause>,
    in_flight_source: Option<LiveWmProposalSource>,
    in_flight: bool,
    cause: LivePublicPolicyCause,
) -> LiveWmRequestAdmission {
    let replaceable_interaction_update = matches!(
        cause.cause,
        sophia_protocol::PolicyRequestCause::Interaction {
            phase: sophia_protocol::PolicyInteractionPhase::Update,
            ..
        }
    );
    if replaceable_interaction_update
        && let Some(pending) = queue.iter_mut().rev().find(|pending| {
            pending.source == cause.source
                && matches!(
                    pending.cause,
                    sophia_protocol::PolicyRequestCause::Interaction {
                        phase: sophia_protocol::PolicyInteractionPhase::Update,
                        ..
                    }
                )
        })
    {
        *pending = cause;
        return LiveWmRequestAdmission::Duplicate;
    }
    if !matches!(
        cause.source,
        LiveWmProposalSource::Action(_) | LiveWmProposalSource::PointerGesture { .. }
    ) && (in_flight_source == Some(cause.source)
        || queue.iter().any(|pending| pending.source == cause.source))
    {
        return LiveWmRequestAdmission::Duplicate;
    }
    if queue.len().saturating_add(usize::from(in_flight)) >= WM_OWNER_REQUEST_CAPACITY {
        return LiveWmRequestAdmission::RejectedCapacity;
    }
    queue.push_back(cause);
    LiveWmRequestAdmission::Admitted
}

fn enqueue_public_policy_security_cancel(
    queue: &mut VecDeque<LivePublicPolicyCause>,
    in_flight: bool,
    cause: LivePublicPolicyCause,
) -> LiveWmRequestAdmission {
    debug_assert!(matches!(
        cause.cause,
        sophia_protocol::PolicyRequestCause::Interaction {
            phase: sophia_protocol::PolicyInteractionPhase::Cancel,
            ..
        }
    ));
    queue.retain(|pending| pending.source != cause.source);
    if queue.len().saturating_add(usize::from(in_flight)) >= WM_OWNER_REQUEST_CAPACITY
        && let Some(index) = queue.iter().rposition(|pending| {
            matches!(pending.source, LiveWmProposalSource::Relayout)
                || matches!(
                    pending.cause,
                    sophia_protocol::PolicyRequestCause::Interaction {
                        phase: sophia_protocol::PolicyInteractionPhase::Update,
                        ..
                    }
                )
        })
    {
        queue.remove(index);
    }
    if queue.len().saturating_add(usize::from(in_flight)) >= WM_OWNER_REQUEST_CAPACITY {
        return LiveWmRequestAdmission::RejectedCapacity;
    }
    queue.push_front(cause);
    LiveWmRequestAdmission::Admitted
}

fn policy_profile_identity(
    connection_epoch: u64,
    key: sophia_config::DesktopProfileActivationKey,
) -> Result<sophia_protocol::WmV1ProfileIdentity, Box<dyn std::error::Error>> {
    sophia_protocol::WmV1ProfileIdentity::new(
        connection_epoch,
        key.generation().raw(),
        key.digest().bytes(),
    )
    .map_err(|error| format!("desktop profile identity is invalid: {error:?}").into())
}

fn bind_public_policy_transport(
    directory: &PolicySessionDirectory,
    profile_key: Option<sophia_config::DesktopProfileActivationKey>,
) -> Result<sophia_runtime::PolicyWmSessionTransport, Box<dyn std::error::Error>> {
    let expected_uid = rustix::process::geteuid().as_raw();
    if profile_key.is_some() {
        return Ok(
            sophia_runtime::PolicyWmSessionTransport::bind_for_supervised_uid_profile_activation(
                directory.endpoint_path(),
                expected_uid,
            )?,
        );
    }
    Ok(
        sophia_runtime::PolicyWmSessionTransport::bind_for_supervised_uid(
            directory.endpoint_path(),
            expected_uid,
        )?,
    )
}

fn start_public_policy_worker(
    transport: sophia_runtime::PolicyWmSessionTransport,
    connection_epoch: u64,
    profile_key: Option<sophia_config::DesktopProfileActivationKey>,
) -> Result<PolicyTransportWorker, Box<dyn std::error::Error>> {
    match profile_key {
        Some(key) => Ok(PolicyTransportWorker::new_profile_activated(
            transport,
            connection_epoch,
            policy_profile_identity(connection_epoch, key)?,
            TransactionId::from_raw(1),
            TransactionId::from_raw(2),
        )?),
        None => Ok(PolicyTransportWorker::new(
            transport,
            connection_epoch,
        )?),
    }
}

impl PreparedPublicPolicyLaunch {
    fn new(config: &PersistentXtermSessionConfig) -> Result<Self, Box<dyn std::error::Error>> {
        let directory = PolicySessionDirectory::create(
            config.wm_socket_path.with_extension("policy"),
        )?;
        let profile_fragments =
            sophia_config::stage_desktop_profile(&config.desktop_profile, directory.path())?;
        sophia_config::validate_desktop_profile_fragments(
            &profile_fragments,
            sophia_config::DesktopProfileActivationKey::from(&config.desktop_profile),
        )?;
        let key = sophia_config::DesktopProfileActivationKey::from(&config.desktop_profile);
        let policy_profile = PreparedAuthorityFragment::new(
            &profile_fragments,
            sophia_config::DesktopAuthority::Policy,
            key,
        )?;
        let shell_profile = PreparedAuthorityFragment::new(
            &profile_fragments,
            sophia_config::DesktopAuthority::Shell,
            key,
        )?;
        let shortcut_profile_slot = sophia_config::DesktopProfileCandidateSlot::with_candidate(
            config.shortcut_profile_candidate.clone(),
        )?;
        let broker_profile = PreparedAuthorityFragment::new(
            &profile_fragments,
            sophia_config::DesktopAuthority::Broker,
            key,
        )?;
        Ok(Self {
            profile_fragments,
            directory,
            policy_profile,
            shell_profile,
            shortcut_profile_slot,
            broker_profile,
        })
    }

    fn start_runtime(
        &self,
        config: &PersistentXtermSessionConfig,
        process: &str,
        profile_key: Option<sophia_config::DesktopProfileActivationKey>,
    ) -> Result<StartedPublicPolicyRuntime, Box<dyn std::error::Error>> {
        let mut transport = bind_public_policy_transport(&self.directory, profile_key)?;
        let socket_path = transport.socket_path().to_path_buf();
        let mut output_transport = config
            .native_scanout
            .then(|| {
                sophia_runtime::OutputSessionTransport::bind_for_supervised_uid(
                    self.directory.path().join("output-endpoint"),
                    rustix::process::geteuid().as_raw(),
                )
            })
            .transpose()?;
        let output_socket_path = output_transport
            .as_ref()
            .map(|transport| transport.socket_path().to_path_buf());
        let checkpoint_path = self.directory.checkpoint_path();
        let spec = public_policy_launch_spec(
            config,
            process,
            &socket_path,
            &checkpoint_path,
            self.profile_fragments
                .path(sophia_config::DesktopAuthority::Policy),
            profile_key.is_some(),
            output_socket_path.as_deref(),
        )?;
        let mut supervisor = ProcessSupervisor::new(SupervisedProcessKind::WindowManager, spec);
        let restart_policy = RestartPolicy::default();
        let mut supervisor_state =
            sophia_runtime::SupervisorState::new(SupervisedProcessKind::WindowManager);
        let (state, command) = update_supervisor(
            supervisor_state,
            SupervisorEvent::StartRequested,
            restart_policy,
        );
        supervisor_state = state;
        let started = supervisor
            .apply(command)?
            .ok_or("public WM supervisor did not start Hagia")?;
        let child_pid = supervisor
            .peer_id()
            .ok_or("public WM supervisor did not retain Hagia's PID")?;
        transport.authorize_supervised_pid(child_pid)?;
        if let Some(output_transport) = output_transport.as_mut() {
            output_transport.authorize_supervised_pid(child_pid)?;
        }
        let (state, _) = update_supervisor(supervisor_state, started, restart_policy);
        supervisor_state = state;
        let worker = start_public_policy_worker(transport, 1, profile_key)?;
        Ok(StartedPublicPolicyRuntime {
            supervisor,
            supervisor_state,
            restart_policy,
            worker,
            output_transport,
            socket_path,
            checkpoint_path,
        })
    }

    fn into_started(
        self,
        runtime: StartedPublicPolicyRuntime,
        profile_key: Option<sophia_config::DesktopProfileActivationKey>,
    ) -> StartedPublicPolicyLaunch {
        let Self {
            profile_fragments,
            directory,
            policy_profile,
            shell_profile,
            shortcut_profile_slot,
            broker_profile,
        } = self;
        StartedPublicPolicyLaunch {
            runtime,
            profile_fragments,
            policy_profile,
            shell_profile,
            shortcut_profile_slot,
            broker_profile,
            profile_key,
            directory,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PublicPolicyRestartDecision {
    Idle,
    AbortSettlement,
    Restart,
}

const fn public_policy_restart_decision(
    restart_requested: bool,
    process_exited: bool,
    settlement_pending: bool,
) -> PublicPolicyRestartDecision {
    if !restart_requested && !process_exited {
        PublicPolicyRestartDecision::Idle
    } else if settlement_pending {
        PublicPolicyRestartDecision::AbortSettlement
    } else {
        PublicPolicyRestartDecision::Restart
    }
}

const fn public_policy_restart_settlement_pending(
    layout_settlement_pending: bool,
    output_effect_dispatched: bool,
) -> bool {
    layout_settlement_pending || output_effect_dispatched
}

include!("public_policy/output_responses.rs");
include!("public_policy/output_publication.rs");
include!("public_policy/projection.rs");

fn public_policy_surface_snapshots(
    layout: &PersistentLiveLayout,
    current_output: &BTreeMap<SurfaceId, sophia_protocol::OutputId>,
    committed_geometry: &BTreeMap<SurfaceId, Rect>,
    committed_presentation: &BTreeMap<
        SurfaceId,
        sophia_protocol::PolicyPresentationState,
    >,
    chrome: sophia_engine::SurfaceChromeStyle,
) -> Result<Vec<sophia_protocol::PolicySurfaceSnapshot>, Box<dyn std::error::Error>> {
    // Retained pixels outlive withdrawal. Only current authority/planning
    // facts grant a surface standing in the WM snapshot.
    let mut surface_ids = layout
        .planning_surfaces
        .keys()
        .chain(layout.authority_surface_facts.keys())
        .copied()
        .collect::<BTreeSet<_>>();
    surface_ids.retain(|surface| {
        layout.is_policy_managed(*surface)
            && layout.client_routes.client_for_surface(*surface).is_some()
            && (layout.mapped_surfaces.contains(surface)
                || layout.planning_surfaces.contains_key(surface)
                || layout.admissions.state(*surface)
                    != sophia_engine::SurfacePresentationAdmissionState::Inactive)
    });
    let mut surfaces = Vec::with_capacity(surface_ids.len());
    for surface in surface_ids {
        let facts = layout
            .layout_facts(surface)
            .ok_or("public WM scene lost a known surface")?;
        // `LayerSnapshot::generation` identifies committed raster content. It
        // may advance on every client repaint without changing a single fact
        // the spatial policy can act on. The public protocol field is instead
        // the authority's window-state generation: using the raster identity
        // here made ordinary Kitty drawing retire an in-flight layout as stale
        // and forced a stateful policy client rebuild for nearly every frame.
        let state_generation = layout
            .authority_surface_facts
            .get(&surface)
            .map(|facts| facts.generation)
            .unwrap_or(facts.generation);
        let kind = match facts.kind {
            sophia_protocol::LayoutNodeKind::Toplevel => {
                sophia_protocol::PolicySurfaceKind::Toplevel
            }
            sophia_protocol::LayoutNodeKind::Dialog => {
                sophia_protocol::PolicySurfaceKind::Dialog
            }
            sophia_protocol::LayoutNodeKind::Utility => {
                sophia_protocol::PolicySurfaceKind::Utility
            }
            sophia_protocol::LayoutNodeKind::Popup => sophia_protocol::PolicySurfaceKind::Popup,
            sophia_protocol::LayoutNodeKind::Unknown => {
                sophia_protocol::PolicySurfaceKind::Unknown
            }
        };
        surfaces.push(sophia_protocol::PolicySurfaceSnapshot {
            surface,
            generation: state_generation.max(1),
            current_output: current_output.get(&surface).copied(),
            kind,
            capabilities: sophia_protocol::LayoutNodeCapabilities::STANDARD_TOPLEVEL,
            constraints: sophia_engine::outer_surface_constraints(facts.constraints, chrome)?,
            exact_size: None,
            requested_state: committed_presentation
                .get(&surface)
                .copied()
                .unwrap_or_default(),
            current_state: committed_presentation
                .get(&surface)
                .copied()
                .unwrap_or_default(),
            transient_owner: facts.presentation_owner,
            geometry: committed_geometry
                .get(&surface)
                .copied()
                .map(Ok)
                .unwrap_or_else(|| sophia_engine::outer_surface_geometry(facts.geometry, chrome))?,
        });
    }
    surfaces.sort_by_key(|surface| surface.surface);
    Ok(surfaces)
}

include!("public_policy/output_facade.rs");

impl Drop for LivePublicPolicyState {
    fn drop(&mut self) {
        for (_, ticket) in std::mem::take(&mut self.control_tickets) {
            ticket.finish(if ticket.dispatched() { sophia_protocol::ControlOutcome::Indeterminate } else { sophia_protocol::ControlOutcome::Stale });
        }
        // The checkpoint parent outlives each peer endpoint so supervised
        // replacement can preserve private policy state. Drop the endpoint
        // worker first, then remove the checkpoint and its session directory.
        self.worker.take();
        let _ = std::fs::remove_file(&self.checkpoint_path);
    }
}

fn observe_public_output_generations(
    generations: &mut BTreeMap<sophia_protocol::OutputId, u64>,
    live: &mut BTreeSet<sophia_protocol::OutputId>,
    outputs: &[sophia_engine::HeadlessOutput],
) -> Result<(), Box<dyn std::error::Error>> {
    let next = outputs.iter().map(|output| output.id).collect::<BTreeSet<_>>();
    for output in next.difference(live) {
        let generation = generations.entry(*output).or_insert(0);
        *generation = generation
            .checked_add(1)
            .ok_or("public WM output generation exhausted")?;
    }
    *live = next;
    Ok(())
}

fn public_session_operations(
    config: &PersistentXtermSessionConfig,
) -> (
    Vec<sophia_protocol::PolicySessionOperation>,
    BTreeMap<u64, WmSessionAction>,
) {
    let issuer = NEXT_POLICY_OPERATION_ISSUER.fetch_add(1, Ordering::Relaxed);
    assert!(
        issuer != 0 && issuer <= (u64::MAX >> 16),
        "public policy operation issuer identity exhausted"
    );
    let token = |slot: u16| (issuer << 16) | u64::from(slot);
    let mut operations = Vec::new();
    let mut actions = BTreeMap::new();
    let mut admit = |slot: u16, token: u64, action: WmSessionAction, target: bool| {
        operations.push(sophia_protocol::PolicySessionOperation {
            token,
            slot,
            permits_surface_target: target,
        });
        actions.insert(token, action);
    };
    if !config.normal_session || config.applications.terminal.is_some() {
        admit(
            1,
            token(1),
            WmSessionAction::LaunchApplication {
                application: TERMINAL_APPLICATION_ID,
            },
            false,
        );
    }
    if config.normal_session && config.applications.browser.is_some() {
        admit(
            2,
            token(2),
            WmSessionAction::LaunchApplication {
                application: BROWSER_APPLICATION_ID,
            },
            false,
        );
    }
    admit(3, token(3), WmSessionAction::CloseFocused, true);
    if config.applications.logout_enabled {
        admit(4, token(4), WmSessionAction::Logout, false);
    }
    // Reloading the profile and replacing the policy client are always
    // available. Neither depends on a configured application, and a desktop
    // whose configuration is wrong is exactly the one that needs them.
    admit(5, token(5), WmSessionAction::ReloadProfile, false);
    admit(6, token(6), WmSessionAction::RestartWm, false);
    if config.application_catalog.is_some() {
        admit(7, token(7), WmSessionAction::LaunchApplication { application: LAUNCHER_APPLICATION_ID }, false);
    }
    (operations, actions)
}

fn public_policy_launch_spec(
    config: &PersistentXtermSessionConfig,
    process: &str,
    socket_path: &std::path::Path,
    checkpoint_path: &std::path::Path,
    candidate_path: &std::path::Path,
    require_profile_activation: bool,
    output_socket_path: Option<&std::path::Path>,
) -> Result<ProcessLaunchSpec, sophia_runtime::ProtectionDomainSpecError> {
    let spec = ProcessLaunchSpec::new(process)
        .env(sophia_runtime::SOPHIA_WM_SOCKET_ENV, socket_path)
        .env("HAGIA_POLICY_CHECKPOINT", checkpoint_path)
        .env("HAGIA_POLICY_CANDIDATE", candidate_path)
        .process_group();
    let spec = if let Some(output_socket_path) = output_socket_path {
        spec.env(
            sophia_runtime::SOPHIA_OUTPUT_SOCKET_ENV,
            output_socket_path,
        )
    } else {
        spec
    };
    let spec = if require_profile_activation {
        spec.env("HAGIA_POLICY_PROFILE_ACTIVATION", "required")
    } else {
        spec
    };
    let spec = config.wm_process_args.iter().fold(
        spec,
        |spec, argument| spec.arg(argument),
    );
    let roles = if output_socket_path.is_some() {
        vec![
            sophia_runtime::ProtectionDomainRole::SpatialPolicy,
            sophia_runtime::ProtectionDomainRole::OutputAuthority,
        ]
    } else {
        vec![sophia_runtime::ProtectionDomainRole::SpatialPolicy]
    };
    let mut domain = sophia_runtime::ProtectionDomainSpec::bubblewrap(roles)?
        .path(sophia_runtime::ProtectionPath::read_only(candidate_path))?
        .path(sophia_runtime::ProtectionPath::read_only(
            socket_path
                .parent()
                .expect("a public policy socket always has a parent"),
        ))?
        .path(sophia_runtime::ProtectionPath::read_write(
            checkpoint_path
                .parent()
                .expect("a public policy checkpoint always has a parent"),
        ))?;
    if let Some(output_socket_path) = output_socket_path {
        domain = domain.path(sophia_runtime::ProtectionPath::read_only(
            output_socket_path
                .parent()
                .expect("an output authority socket always has a parent"),
        ))?;
    }
    for executable in &config.wm_process_executable_grants {
        domain = domain.path(sophia_runtime::ProtectionPath::read_only(executable))?;
    }
    Ok(spec.protection_domain(domain))
}

include!("public_policy/session_start.rs");
include!("public_policy/requests.rs");
include!("public_policy/restart.rs");
include!("public_policy/work_areas.rs");
include!("public_policy/commit_and_proof.rs");

include!("public_policy/proposal.rs");

include!("profile_reload.rs");
