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
    in_flight_origin_surfaces: Vec<SurfaceId>,
    outputs: Vec<sophia_engine::HeadlessOutput>,
    output_bounds: BTreeMap<sophia_protocol::OutputId, Rect>,
    output_generations: BTreeMap<sophia_protocol::OutputId, u64>,
    live_output_ids: BTreeSet<sophia_protocol::OutputId>,
    work_areas: BTreeMap<sophia_protocol::OutputId, Rect>,
    session_operations: Vec<sophia_protocol::PolicySessionOperation>,
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

impl LivePublicPolicyState {
    fn poll_output_authority(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // The transport may already have admitted a replacement peer after the
        // old one vanished. Leave its bounded event queue untouched until the
        // physical rollback settles and the authority owner adopts the new
        // connection epoch; otherwise a valid proposal against the preserved
        // snapshot is spuriously rejected as stale.
        if self.output_cancel_requested.is_some() {
            return Ok(());
        }
        const MAX_EVENTS_PER_TURN: usize = 16;
        for _ in 0..MAX_EVENTS_PER_TURN {
            let event = match self.output_service.as_ref() {
                Some(service) => match service.try_event() {
                    Ok(Some(event)) => event,
                    Ok(None) => break,
                    Err(_disconnected) => {
                        self.output_service.take();
                        crate::session_println!(
                            "sophia_live_output_authority schema=1 status=degraded reason=service_disconnected preserved_topology=true"
                        );
                        break;
                    }
                },
                None => break,
            };
            match event {
                sophia_runtime::OutputTransportServiceEvent::Connected { connection_epoch } => {
                    let authority = self
                        .output_authority
                        .as_mut()
                        .ok_or("output service connected without an authority owner")?;
                    if connection_epoch > authority.connection_epoch() {
                        if authority.active_transaction().is_some() {
                            self.output_pending_connection_epoch = Some(
                                self.output_pending_connection_epoch
                                    .map_or(connection_epoch, |pending| {
                                        pending.max(connection_epoch)
                                    }),
                            );
                        } else {
                            authority.replace_connection_epoch(connection_epoch)?;
                        }
                    } else if connection_epoch != authority.connection_epoch() {
                        return Err("output service connected with a stale epoch".into());
                    }
                    crate::session_println!(
                        "sophia_live_output_authority schema=1 status=connected epoch={connection_epoch}"
                    );
                }
                sophia_runtime::OutputTransportServiceEvent::Proposal {
                    proposal,
                    admission,
                } => match admission {
                    sophia_runtime::OutputProposalAdmission::Active => {
                        self.settle_output_proposal(proposal)?;
                    }
                    sophia_runtime::OutputProposalAdmission::Queued { replaced } => {
                        if let Some(replaced) = replaced {
                            let authority = self
                                .output_authority
                                .as_ref()
                                .ok_or("queued output proposal has no authority owner")?;
                            self.output_service
                                .as_ref()
                                .ok_or("queued output proposal lost its service")?
                                .command(sophia_runtime::OutputTransportServiceCommand::Reply {
                                    transaction: replaced.transaction,
                                    outcome: sophia_protocol::OutputV1Outcome {
                                        connection_epoch: authority.connection_epoch(),
                                        topology_epoch: authority.published().topology_epoch,
                                        kind: sophia_protocol::OutputV1OutcomeKind::Stale,
                                        reason: sophia_protocol::SOPHIA_OUTPUT_OUTCOME_REASON_STALE,
                                    },
                                })
                                .map_err(|_| "output stale-reply queue disconnected")?;
                        }
                    }
                },
                sophia_runtime::OutputTransportServiceEvent::Promoted(proposal) => {
                    self.settle_output_proposal(proposal)?;
                }
                sophia_runtime::OutputTransportServiceEvent::ProposalRejected {
                    transaction,
                    message,
                } => {
                    crate::session_println!(
                        "sophia_live_output_authority schema=1 status=rejected transaction={} phase=admission reason={message:?}",
                        transaction.raw(),
                    );
                }
                sophia_runtime::OutputTransportServiceEvent::Disconnected {
                    connection_epoch,
                } => {
                    let replacement_epoch = connection_epoch
                        .checked_add(1)
                        .ok_or("output connection epoch exhausted after disconnect")?;
                    self.request_output_candidate_cancellation(
                        format!("output peer disconnected at epoch {connection_epoch}"),
                        Some(replacement_epoch),
                    )?;
                    crate::session_println!(
                        "sophia_live_output_authority schema=1 status=disconnected epoch={connection_epoch} preserved_topology=true"
                    );
                    break;
                }
                sophia_runtime::OutputTransportServiceEvent::AssigneeReplaced {
                    connection_epoch,
                    abandoned,
                } => {
                    self.request_output_candidate_cancellation(
                        format!("output assignee replaced at epoch {connection_epoch}"),
                        Some(connection_epoch),
                    )?;
                    crate::session_println!(
                        "sophia_live_output_authority schema=1 status=reauthorized epoch={} abandoned={} preserved_topology=true",
                        connection_epoch,
                        abandoned.len(),
                    );
                    if self.output_cancel_requested.is_some() {
                        break;
                    }
                }
                sophia_runtime::OutputTransportServiceEvent::ConnectionRejected { message } => {
                    crate::session_println!(
                        "sophia_live_output_authority schema=1 status=connection_rejected reason={message:?} preserved_topology=true"
                    );
                }
                sophia_runtime::OutputTransportServiceEvent::Failed { message } => {
                    self.request_output_candidate_cancellation(
                        format!("output authority service failed: {message}"),
                        None,
                    )?;
                    self.output_service.take();
                    crate::session_println!(
                        "sophia_live_output_authority schema=1 status=degraded reason={message:?} preserved_topology=true"
                    );
                    break;
                }
            }
        }
        Ok(())
    }

    fn settle_output_proposal(
        &mut self,
        proposal: sophia_runtime::AdmittedOutputProposal,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let admission = {
            let authority = self
                .output_authority
                .as_mut()
                .ok_or("output proposal has no authority owner")?;
            authority.admit(
                proposal.transaction,
                &proposal.message,
                &self.output_capabilities,
            )
        };
        let settlement = match admission {
            Ok(crate::live_output_authority::LiveOutputAuthorityAdmission::Validated(
                settlement,
            )) => settlement,
            Ok(crate::live_output_authority::LiveOutputAuthorityAdmission::Prepared) => {
                self.output_effect_dispatched = false;
                crate::session_println!(
                    "sophia_live_output_authority schema=1 status=effect_pending transaction={} preserved_topology=true",
                    proposal.transaction.raw(),
                );
                return Ok(());
            }
            Err(error) => {
                let authority = self
                    .output_authority
                    .as_ref()
                    .ok_or("output admission failure lost its authority owner")?;
                tracing::warn!(
                    "sophia_live_output_authority schema=1 status=rejected transaction={} phase=admission error={error}",
                    proposal.transaction.raw(),
                );
                crate::live_output_authority::LiveOutputAuthoritySettlement {
                    transaction: proposal.transaction,
                    outcome: sophia_protocol::OutputV1Outcome {
                        connection_epoch: authority.connection_epoch(),
                        topology_epoch: authority.published().topology_epoch,
                        kind: sophia_protocol::OutputV1OutcomeKind::Rejected,
                        reason: sophia_protocol::SOPHIA_OUTPUT_OUTCOME_REASON_INVARIANT,
                    },
                    published_snapshot: None,
                }
            }
        };
        self.send_output_settlement(settlement)
    }

    fn send_output_settlement(
        &self,
        settlement: crate::live_output_authority::LiveOutputAuthoritySettlement,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.output_service
            .as_ref()
            .ok_or("output settlement lost its transport service")?
            .command(sophia_runtime::OutputTransportServiceCommand::Settle {
                transaction: settlement.transaction,
                outcome: settlement.outcome,
            })
            .map_err(|_| "output settlement queue disconnected")?;
        crate::session_println!(
            "sophia_live_output_authority schema=1 status=settled transaction={} outcome={:?} topology_epoch={}",
            settlement.transaction.raw(),
            settlement.outcome.kind,
            settlement.outcome.topology_epoch,
        );
        Ok(())
    }

    fn finish_output_settlement(
        &mut self,
        settlement: crate::live_output_authority::LiveOutputAuthoritySettlement,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let startup = self.startup_output_transaction == Some(settlement.transaction);
        let cancelled = self
            .output_cancel_requested
            .as_ref()
            .is_some_and(|(transaction, _)| *transaction == settlement.transaction);
        let local_reason = cancelled
            .then(|| {
                self.output_cancel_requested
                    .as_ref()
                    .expect("matching output cancellation remains recorded")
                    .1
                    .clone()
            })
            .or_else(|| startup.then(|| "desktop profile startup".to_owned()));
        let publish_committed = (settlement.outcome.kind
            == sophia_protocol::OutputV1OutcomeKind::Committed)
            .then(|| settlement.published_snapshot.clone())
            .flatten();
        if let Some(reason) = local_reason {
            if let Some(connection_epoch) = self.output_pending_connection_epoch {
                let mut replacement = self
                    .output_authority
                    .as_ref()
                    .ok_or("cancelled output settlement lost its authority owner")?
                    .clone();
                replacement.replace_connection_epoch(connection_epoch)?;
                self.output_authority = Some(replacement);
            }
            if cancelled {
                self.output_cancel_requested = None;
            }
            if startup {
                self.startup_output_transaction = None;
            }
            self.output_pending_connection_epoch = None;
            crate::session_println!(
                "sophia_live_output_authority schema=3 status=settled_locally transaction={} outcome={:?} topology_epoch={} reason={reason:?} preserved_topology={}",
                settlement.transaction.raw(),
                settlement.outcome.kind,
                settlement.outcome.topology_epoch,
                publish_committed.is_none(),
            );
        } else if let Err(error) = self.send_output_settlement(settlement.clone()) {
            // The reducer is already terminal. In particular, a committed
            // topology has crossed physical first presentation and cannot be
            // made private again because its peer vanished between owner turns.
            self.output_service.take();
            tracing::warn!(
                "sophia_live_output_authority schema=2 status=degraded reason=terminal_settlement_transport transaction={} outcome={:?} error={error} preserved_topology=true",
                settlement.transaction.raw(),
                settlement.outcome.kind,
            );
        }
        // A committed topology is the desk from now on, so the transport's copy
        // has to become it: only the client that submitted learns the new epoch
        // from its outcome, and everyone who connects later -- a restarted
        // policy, a second tool -- is answered from the service's stored
        // snapshot. It goes out after the settlement, never before. A snapshot
        // is an unsolicited update and an outcome is the answer to a request;
        // sending the update first put a frame the client was not waiting for
        // in front of the one it was, and it failed to decode it.
        if let Some(published) = publish_committed {
            let topology_epoch = published.topology_epoch;
            let (transaction, transport_published) =
                self.publish_snapshot_to_transport(published, "committed_snapshot_transport")?;
            crate::session_println!(
                "sophia_live_output_authority schema=2 status=committed_snapshot_published transaction={} topology_epoch={topology_epoch} transport_published={transport_published}",
                transaction.raw(),
            );
        }
        Ok(())
    }

    fn request_output_candidate_cancellation(
        &mut self,
        reason: String,
        replacement_epoch: Option<u64>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(connection_epoch) = replacement_epoch {
            self.output_pending_connection_epoch = Some(
                self.output_pending_connection_epoch
                    .map_or(connection_epoch, |pending| pending.max(connection_epoch)),
            );
        }
        let startup_active = self.startup_output_transaction.is_some_and(|startup| {
            self.output_authority
                .as_ref()
                .and_then(|authority| authority.active_transaction())
                == Some(startup)
        });
        if startup_active {
            tracing::warn!(
                "sophia_live_output_authority schema=3 status=startup_peer_loss_ignored reason={reason:?} preserved_candidate=true"
            );
            return Ok(());
        }
        if self.output_effect_dispatched {
            let transaction = self
                .output_authority
                .as_ref()
                .and_then(|authority| authority.active_transaction())
                .ok_or("dispatched output effect lost its authority transaction")?;
            match self.output_cancel_requested.as_ref() {
                Some((pending, _)) if *pending != transaction => {
                    return Err("output cancellation targets a different transaction".into());
                }
                Some(_) => {}
                None => self.output_cancel_requested = Some((transaction, reason)),
            }
            return Ok(());
        }
        self.abandon_output_candidate()?;
        if let Some(connection_epoch) = self.output_pending_connection_epoch.take() {
            self.output_authority
                .as_mut()
                .ok_or("output assignee replacement has no authority owner")?
                .replace_connection_epoch(connection_epoch)?;
        }
        Ok(())
    }

    /// Whether an output policy candidate is dispatched or being cancelled.
    ///
    /// Publishing a hardware snapshot in either state is the race that
    /// `publish_output_authority_snapshot` refuses, so callers holding one ask
    /// here rather than discovering it as a session-ending error.
    fn output_candidate_active(&self) -> bool {
        self.output_authority
            .as_ref()
            .is_some_and(|authority| authority.active_transaction().is_some())
            || self.output_cancel_requested.is_some()
    }

    fn output_authority_topology_epoch(&self) -> Option<u64> {
        self.output_authority
            .as_ref()
            .map(|authority| authority.published().topology_epoch)
    }

    fn output_candidate_cancellation_reason(
        &self,
        transaction: TransactionId,
    ) -> Option<&str> {
        self.output_cancel_requested
            .as_ref()
            .filter(|(pending, _)| *pending == transaction)
            .map(|(_, reason)| reason.as_str())
    }

    /// Hands a snapshot to the transport that answers future connections.
    ///
    /// The service keeps its own copy and sends it to whoever connects, so a
    /// snapshot never pushed here is invisible to anyone arriving later --
    /// including a policy the supervisor restarts, which then reasons about a
    /// desk that no longer exists.
    fn publish_snapshot_to_transport(
        &mut self,
        snapshot: sophia_protocol::OutputAuthoritySnapshot,
        degraded_reason: &str,
    ) -> Result<(TransactionId, bool), Box<dyn std::error::Error>> {
        let transaction = TransactionId::from_raw(self.next_output_snapshot_transaction);
        self.next_output_snapshot_transaction = self
            .next_output_snapshot_transaction
            .checked_add(1)
            .ok_or("output snapshot transaction exhausted")?;
        let published = self.output_service.as_ref().is_some_and(|service| {
            service
                .command(sophia_runtime::OutputTransportServiceCommand::PublishSnapshot {
                    transaction,
                    snapshot,
                })
                .is_ok()
        });
        if !published {
            self.output_service.take();
            tracing::warn!(
                "sophia_live_output_authority schema=2 status=degraded reason={degraded_reason} preserved_topology=true"
            );
        }
        Ok((transaction, published))
    }

    fn publish_output_authority_snapshot(
        &mut self,
        snapshot: sophia_protocol::OutputAuthoritySnapshot,
        capabilities: Vec<sophia_backend_live::LibdrmNativeOutputCapability>,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let Some(authority) = self.output_authority.as_ref() else {
            return Ok(false);
        };
        if self.output_candidate_active() {
            return Err("hardware output publication raced an active policy candidate".into());
        }
        let mut replacement = authority.clone();
        replacement.replace_published_snapshot(snapshot.clone())?;
        let (transaction, transport_published) =
            self.publish_snapshot_to_transport(snapshot, "hardware_snapshot_transport")?;
        self.output_authority = Some(replacement);
        self.output_capabilities = capabilities;
        crate::session_println!(
            "sophia_live_output_authority schema=2 status=hardware_snapshot_published transaction={} topology_epoch={} heads={} groups={} first_presented=true transport_published={transport_published}",
            transaction.raw(),
            self.output_authority
                .as_ref()
                .expect("replacement authority installed above")
                .published()
                .topology_epoch,
            self.output_capabilities.len(),
            self.output_authority
                .as_ref()
                .expect("replacement authority installed above")
                .published()
                .groups
                .len(),
        );
        Ok(true)
    }

    fn take_output_topology_effect(
        &mut self,
    ) -> Option<crate::live_output_authority::LiveOutputAuthorityEffect> {
        if self.output_effect_dispatched {
            return None;
        }
        let effect = self.output_authority.as_ref()?.active_effect()?;
        self.output_effect_dispatched = true;
        Some(effect)
    }

    fn published_output_snapshot(&self) -> Option<sophia_protocol::OutputAuthoritySnapshot> {
        self.output_authority
            .as_ref()
            .map(|authority| authority.published().clone())
    }

    fn take_output_topology_reload_request(&mut self) -> bool {
        if self.output_candidate_active() {
            return false;
        }
        std::mem::take(&mut self.output_topology_reload_pending)
    }

    /// Admits a topology a reloaded profile asked for, as an ordinary
    /// candidate.
    ///
    /// This is the same admission the startup effect uses, and it carries no
    /// privilege of its own: the effect it leaves behind is drained, quiesced,
    /// prepared and rolled back by exactly the machinery that already runs at
    /// session start. A reload is not a second way to set a mode, only a second
    /// occasion to use the first one.
    ///
    /// Whether the topology actually differs is decided before this is called,
    /// by comparing the reloaded profile's output values against the running
    /// ones. A reload that changed a keybinding never reaches here, which is
    /// what keeps it from blinking a display.
    ///
    /// Returns whether an effect is now waiting to be drained.
    fn admit_reloaded_output_topology(
        &mut self,
        candidate: sophia_protocol::OutputTopologyCandidate,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let connection_epoch = self.connection_epoch;
        let capabilities = self.output_capabilities.clone();
        let Some(authority) = self.output_authority.as_mut() else {
            crate::session_eprintln!(
                "sophia_live_output_authority schema=3 status=reload_declined reason=no_authority"
            );
            return Ok(false);
        };
        if authority.active_transaction().is_some() {
            // Admission cannot interrupt an owned topology transaction.
            crate::session_eprintln!(
                "sophia_live_output_authority schema=3 status=reload_declined reason=candidate_active"
            );
            return Ok(false);
        }
        let transaction = TransactionId::from_raw(self.next_output_snapshot_transaction);
        self.next_output_snapshot_transaction =
            self.next_output_snapshot_transaction.saturating_add(1);
        let admission = match authority.admit(
            transaction,
            &sophia_protocol::OutputV1Proposal {
                connection_epoch,
                candidate,
            },
            &capabilities,
        ) {
            Ok(admission) => admission,
            Err(error) => {
                // A mode the hardware will not take is the operator's typo far
                // more often than it is our defect, so it costs them a log line
                // and not their desktop.
                crate::session_eprintln!(
                    "sophia_live_output_authority schema=3 status=reload_declined reason=not_admitted detail={error}"
                );
                return Ok(false);
            }
        };
        if !matches!(
            admission,
            crate::live_output_authority::LiveOutputAuthorityAdmission::Prepared
        ) {
            crate::session_eprintln!(
                "sophia_live_output_authority schema=3 status=reload_declined reason=not_prepared"
            );
            return Ok(false);
        }
        // The latch is what the startup effect left set. Clearing it is what
        // makes this candidate reachable by the same drain.
        self.output_effect_dispatched = false;
        crate::session_println!(
            "sophia_live_output_authority schema=3 status=reload_effect_pending transaction={}",
            transaction.raw(),
        );
        Ok(true)
    }


    fn output_topology_effect_pending(&self) -> bool {
        !self.output_effect_dispatched
            && self
                .output_authority
                .as_ref()
                .is_some_and(|authority| authority.active_effect().is_some())
    }

    fn ordinary_policy_settlement_idle(&self) -> bool {
        !self.cycle_submitted
            && self.in_flight_request.is_none()
            && self.staged.is_none()
            && self.prepared.is_none()
            && self.pending_operation.is_none()
            && self.deferred_command.is_none()
    }

    fn reject_output_topology_effect(
        &mut self,
        transaction: TransactionId,
        failure: sophia_engine::OutputTopologyTransactionFailure,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let authority = self
            .output_authority
            .as_mut()
            .ok_or("output effect observation has no authority owner")?;
        if authority.active_transaction() != Some(transaction) {
            return Err("output effect observation targets a stale transaction".into());
        }
        let transition = authority.fail(failure)?;
        if matches!(
            transition,
            sophia_engine::OutputTopologyTransactionTransition::OutOfOrder
                | sophia_engine::OutputTopologyTransactionTransition::UnknownHead
                | sophia_engine::OutputTopologyTransactionTransition::UnknownOutput
                | sophia_engine::OutputTopologyTransactionTransition::Terminal
        ) {
            return Err(format!(
                "output effect observation violated transaction order: {transition:?}"
            )
            .into());
        }
        if matches!(
            authority.active_phase(),
            Some(
                sophia_engine::OutputTopologyTransactionPhase::Committed
                    | sophia_engine::OutputTopologyTransactionPhase::RolledBack
                    | sophia_engine::OutputTopologyTransactionPhase::Failed
            )
        ) {
            let settlement = authority.settle_terminal()?;
            self.output_effect_dispatched = false;
            self.finish_output_settlement(settlement)?;
        }
        Ok(())
    }

    fn begin_output_topology_apply(
        &mut self,
        transaction: TransactionId,
        heads: &[sophia_engine::RenderHeadId],
    ) -> Result<(), Box<dyn std::error::Error>> {
        let authority = self
            .output_authority
            .as_mut()
            .ok_or("output apply observation has no authority owner")?;
        if authority.active_transaction() != Some(transaction)
            || authority.mark_prepared_batch(heads)?
                != sophia_engine::OutputTopologyTransactionTransition::PhaseReady
            || authority.begin_apply()?
                != sophia_engine::OutputTopologyTransactionTransition::PhaseReady
        {
            return Err("output apply preparation violated transaction order".into());
        }
        Ok(())
    }

    fn observe_output_topology_applied(
        &mut self,
        transaction: TransactionId,
        heads: &[sophia_engine::RenderHeadId],
    ) -> Result<(), Box<dyn std::error::Error>> {
        let authority = self
            .output_authority
            .as_mut()
            .ok_or("output apply observation has no authority owner")?;
        if authority.active_transaction() != Some(transaction) {
            return Err("output apply observation targets a stale transaction".into());
        }
        let transition = authority.mark_applied_batch(heads)?;
        if !matches!(
            transition,
            sophia_engine::OutputTopologyTransactionTransition::Accepted
                | sophia_engine::OutputTopologyTransactionTransition::PhaseReady
        ) {
            return Err("output apply observation violated transaction order".into());
        }
        Ok(())
    }

    fn observe_output_topology_first_presented(
        &mut self,
        transaction: TransactionId,
        outputs: &[sophia_protocol::OutputId],
    ) -> Result<Option<sophia_protocol::OutputAuthoritySnapshot>, Box<dyn std::error::Error>> {
        let authority = self
            .output_authority
            .as_mut()
            .ok_or("output presentation observation has no authority owner")?;
        if authority.active_transaction() != Some(transaction)
            || authority.mark_first_presented_batch(outputs)?
                != sophia_engine::OutputTopologyTransactionTransition::PhaseReady
        {
            return Err("output first-presentation observation violated transaction order".into());
        }
        let settlement = authority.settle_terminal()?;
        let published = settlement.published_snapshot.clone();
        self.output_effect_dispatched = false;
        self.finish_output_settlement(settlement)?;
        Ok(published)
    }

    fn preview_output_topology_first_presented(
        &self,
        transaction: TransactionId,
        outputs: &[sophia_protocol::OutputId],
    ) -> Result<sophia_protocol::OutputAuthoritySnapshot, Box<dyn std::error::Error>> {
        let mut authority = self
            .output_authority
            .as_ref()
            .ok_or("output presentation preview has no authority owner")?
            .clone();
        if authority.active_transaction() != Some(transaction)
            || authority.mark_first_presented_batch(outputs)?
                != sophia_engine::OutputTopologyTransactionTransition::PhaseReady
        {
            return Err("output first-presentation preview violated transaction order".into());
        }
        authority
            .settle_terminal()?
            .published_snapshot
            .ok_or_else(|| "output authority preview did not commit a snapshot".into())
    }

    fn observe_output_topology_rolled_back(
        &mut self,
        transaction: TransactionId,
        heads: &[sophia_engine::RenderHeadId],
    ) -> Result<(), Box<dyn std::error::Error>> {
        let authority = self
            .output_authority
            .as_mut()
            .ok_or("output rollback observation has no authority owner")?;
        if authority.active_transaction() != Some(transaction)
            || authority.mark_rolled_back_batch(heads)?
                != sophia_engine::OutputTopologyTransactionTransition::PhaseReady
        {
            return Err("output rollback observation violated transaction order".into());
        }
        let settlement = authority.settle_terminal()?;
        self.output_effect_dispatched = false;
        self.finish_output_settlement(settlement)
    }

    fn observe_output_topology_rollback_failed(
        &mut self,
        transaction: TransactionId,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let authority = self
            .output_authority
            .as_mut()
            .ok_or("output rollback failure has no authority owner")?;
        if authority.active_transaction() != Some(transaction)
            || authority.rollback_failed()?
                != sophia_engine::OutputTopologyTransactionTransition::PhaseReady
        {
            return Err("output rollback failure violated transaction order".into());
        }
        let settlement = authority.settle_terminal()?;
        self.output_effect_dispatched = false;
        self.finish_output_settlement(settlement)
    }

    fn abandon_output_candidate(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let Some(authority) = self.output_authority.as_mut() else {
            return Ok(());
        };
        let active = authority.active_transaction();
        if active.is_some() && active == self.startup_output_transaction {
            return Ok(());
        }
        if active.is_some() {
            authority.fail(sophia_engine::OutputTopologyTransactionFailure::Stale)?;
            let _ = authority.settle_terminal()?;
            self.output_effect_dispatched = false;
        }
        Ok(())
    }

    fn initial_scene(
        outputs: &[sophia_engine::HeadlessOutput],
        active_output: sophia_protocol::OutputId,
        session_operations: Vec<sophia_protocol::PolicySessionOperation>,
    ) -> sophia_protocol::PolicySceneSnapshot {
        let bounds = wm_output_bounds(outputs);
        sophia_protocol::PolicySceneSnapshot {
            generation: 1,
            active_output,
            outputs: bounds
                .into_iter()
                .map(|(output, bounds)| sophia_protocol::PolicyOutputSnapshot {
                    output,
                    generation: 1,
                    focus: None,
                    bounds,
                    work_area: bounds,
                })
                .collect(),
            surfaces: Vec::new(),
            session_operations,
        }
    }

    fn mint_transaction(&mut self) -> Result<TransactionId, Box<dyn std::error::Error>> {
        mint_public_policy_transaction(&mut self.next_transaction)
    }

    fn all_outputs(&self, active: sophia_protocol::OutputId) -> Vec<sophia_protocol::OutputId> {
        let mut outputs = self.outputs.iter().map(|output| output.id).collect::<Vec<_>>();
        outputs.sort_by_key(|output| output.raw());
        if let Some(index) = outputs.iter().position(|output| *output == active) {
            outputs.swap(0, index);
        }
        outputs
    }

    fn queue_cause(&mut self, cause: LivePublicPolicyCause) -> LiveWmRequestAdmission {
        enqueue_public_policy_cause(
            &mut self.queue,
            self.in_flight_source,
            self.in_flight_request.is_some(),
            cause,
        )
    }

    fn queue_security_cancel(
        &mut self,
        cause: LivePublicPolicyCause,
    ) -> LiveWmRequestAdmission {
        enqueue_public_policy_security_cancel(
            &mut self.queue,
            self.in_flight_request.is_some(),
            cause,
        )
    }

    fn admit_dirty(
        &mut self,
        request: sophia_protocol::PolicyDirtyRequest,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if request.connection_epoch != self.connection_epoch || request.affected_outputs.is_empty() {
            return Err("public WM dirty request has an invalid connection or empty scope".into());
        }
        let affected = request
            .affected_outputs
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        if affected.len() != request.affected_outputs.len()
            || !affected.is_subset(&self.live_output_ids)
        {
            return Err("public WM dirty request has duplicate or unknown outputs".into());
        }
        self.reducer
            .admit_policy_generation(request.policy_generation)?;
        self.pending_dirty_outputs.extend(affected);
        Ok(())
    }

    fn materialize_pending_dirty(&mut self) {
        materialize_public_dirty_cause(
            &mut self.queue,
            &mut self.pending_dirty_outputs,
            self.in_flight_source,
        );
    }

    /// Records a terminal projection settlement, re-arming the owner when the
    /// client can only recover from a cycle the owner has to offer.
    ///
    /// Invariant: after rejecting a response the owner never leaves the
    /// connection with no outstanding request and nothing queued. A physical
    /// run stranded exactly there — the client waited for a snapshot that was
    /// never coming and died on its socket deadline, and the resulting restarts
    /// exhausted the supervisor budget.
    fn settle_public_projection(&mut self, outcome: sophia_protocol::PolicyProjectionOutcome) {
        if outcome == sophia_protocol::PolicyProjectionOutcome::Committed
            && let Ok(mut origins) = self.launch_origins.lock()
        {
            origins.publish(self.connection_epoch, &self.staged_launch_contexts);
            origins.committed(self.in_flight_origin_surfaces.iter().copied());
        }
        self.staged_launch_contexts.clear();
        self.in_flight_origin_surfaces.clear();

        if let Some(sophia_protocol::PolicyProjectionRequest {
            cause: sophia_protocol::PolicyRequestCause::Action { activation_serial, .. }, ..
        }) = self.in_flight_request.as_ref()
            && let Some(ticket) = self.control_tickets.remove(activation_serial)
        {
            ticket.finish(match outcome {
                sophia_protocol::PolicyProjectionOutcome::Committed => sophia_protocol::ControlOutcome::Committed,
                sophia_protocol::PolicyProjectionOutcome::RejectedInvalid | sophia_protocol::PolicyProjectionOutcome::RejectedStale => sophia_protocol::ControlOutcome::Rejected,
                _ => sophia_protocol::ControlOutcome::Indeterminate,
            });
        }
        if let Some((surface, _)) = consume_public_launch_classification(
            &mut self.launch_classifications,
            self.in_flight_source,
            outcome,
        )
        {
            crate::session_println!(
                "sophia_session_launch_placement schema=1 status=consumed surface={} metadata=none",
                surface.index(),
            );
        }
        self.cycle_submitted = false;
        self.in_flight_request = None;
        self.in_flight_source = None;
        if public_policy_rearm_after_outcome(outcome) {
            // The whole live set: a stale rejection means the canonical scene
            // moved in a way the owner cannot attribute to particular outputs,
            // and replaying the original cause would replay a user action or
            // name a surface that has just been withdrawn.
            self.pending_dirty_outputs
                .extend(self.live_output_ids.iter().copied());
        }
    }

    fn submit_or_defer(
        &mut self,
        command: PolicyTransportCommand,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self.deferred_command.is_some() {
            return Err("public WM already has a deferred transport command".into());
        }
        if self.transport_unavailable {
            return Ok(());
        }
        let worker = self
            .worker
            .as_ref()
            .ok_or("public WM transport is unavailable")?;
        if let Err(command) = worker.try_command(command) {
            self.deferred_command = Some(command);
        }
        Ok(())
    }

    fn flush_deferred_command(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let Some(command) = self.deferred_command.take() else {
            return Ok(());
        };
        if self.transport_unavailable {
            return Ok(());
        }
        let worker = self
            .worker
            .as_ref()
            .ok_or("public WM transport is unavailable")?;
        if let Err(command) = worker.try_command(command) {
            self.deferred_command = Some(command);
        }
        Ok(())
    }

    fn settle_rejected_projection(
        &mut self,
        projection: &sophia_protocol::PolicyProjectionProposal,
        outcome: sophia_protocol::PolicyProjectionOutcome,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let _ = self.reducer.timeout(projection.request_id);
        self.submit_or_defer(PolicyTransportCommand::ProjectionOutcome {
            transaction: projection.transaction,
            request_id: projection.request_id,
            scene_generation: self.reducer.scene().generation,
            outcome,
            expect_session_operation: false,
        })?;
        self.settle_public_projection(outcome);
        self.expected_operation_slot = None;
        self.staged = None;
        Ok(())
    }

    fn snapshot(
        &self,
        layout: &PersistentLiveLayout,
        chrome: sophia_engine::SurfaceChromeStyle,
    ) -> Result<sophia_protocol::PolicySceneSnapshot, Box<dyn std::error::Error>> {
        let previous = self.reducer.scene();
        let committed = self.reducer.committed();
        let mut current_output = BTreeMap::new();
        let mut committed_geometry = BTreeMap::new();
        let mut committed_presentation = BTreeMap::new();
        for projection in &committed {
            for placement in &projection.placements {
                current_output.insert(placement.surface, projection.output);
                committed_geometry.insert(placement.surface, placement.geometry);
                committed_presentation.insert(placement.surface, placement.presentation);
            }
        }
        let surfaces = public_policy_surface_snapshots(
            layout,
            &current_output,
            &committed_geometry,
            &committed_presentation,
            chrome,
        )?;
        crate::session_println!(
            "sophia_live_wm_snapshot schema=1 status=complete surfaces={} minimized={} unassigned={}",
            surfaces.len(),
            surfaces
                .iter()
                .filter(|surface| surface.current_state.minimized)
                .count(),
            surfaces
                .iter()
                .filter(|surface| surface.current_output.is_none())
                .count(),
        );
        let outputs = self
            .outputs
            .iter()
            .map(|descriptor| descriptor.id)
            .map(|output| {
                let bounds = self
                    .output_bounds
                    .get(&output)
                    .copied()
                    .ok_or("public WM snapshot lost logical output bounds")?;
                Ok(sophia_protocol::PolicyOutputSnapshot {
                    output,
                    generation: self.output_generations.get(&output).copied().unwrap_or(1),
                    focus: public_policy_snapshot_focus(
                        output,
                        committed
                            .iter()
                            .find(|projection| projection.output == output)
                            .and_then(|projection| projection.focus),
                        &surfaces,
                    ),
                    bounds,
                    work_area: self.work_areas.get(&output).copied().unwrap_or(bounds),
                })
            })
            .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?;
        let mut candidate = sophia_protocol::PolicySceneSnapshot {
            generation: previous.generation,
            active_output: self.active_output,
            outputs,
            surfaces,
            session_operations: self.session_operations.clone(),
        };
        let same_facts = candidate.active_output == previous.active_output
            && candidate.outputs == previous.outputs
            && candidate.surfaces == previous.surfaces
            && candidate.session_operations == previous.session_operations;
        if !same_facts {
            candidate.generation = previous
                .generation
                .checked_add(1)
                .ok_or("public WM scene generation exhausted")?;
        }
        Ok(candidate)
    }
}

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

impl LiveWmSession {
    fn poll_output_authority(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(public) = self.public.as_mut() {
            public.poll_output_authority()?;
        }
        Ok(())
    }

    fn output_topology_effect_pending(&self) -> bool {
        self.public
            .as_ref()
            .is_some_and(LivePublicPolicyState::output_topology_effect_pending)
    }

    fn startup_output_topology_pending(&self) -> bool {
        self.public.as_ref().is_some_and(|public| public.startup_output_transaction.is_some())
    }

    fn is_startup_output_transaction(&self, transaction: TransactionId) -> bool {
        self.public
            .as_ref()
            .is_some_and(|public| public.startup_output_transaction == Some(transaction))
    }

    fn ordinary_policy_settlement_idle(&self) -> bool {
        self.public
            .as_ref()
            .is_none_or(LivePublicPolicyState::ordinary_policy_settlement_idle)
    }

    fn take_output_topology_effect(
        &mut self,
    ) -> Option<crate::live_output_authority::LiveOutputAuthorityEffect> {
        self.public.as_mut()?.take_output_topology_effect()
    }

    fn take_output_topology_reload_request(&mut self) -> bool {
        self.public
            .as_mut()
            .is_some_and(LivePublicPolicyState::take_output_topology_reload_request)
    }

    fn published_output_snapshot(&self) -> Option<sophia_protocol::OutputAuthoritySnapshot> {
        self.public.as_ref()?.published_output_snapshot()
    }

    fn admit_reloaded_output_topology(
        &mut self,
        candidate: sophia_protocol::OutputTopologyCandidate,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        match self.public.as_mut() {
            Some(public) => public.admit_reloaded_output_topology(candidate),
            None => Ok(false),
        }
    }

    fn output_topology_cancellation_reason(
        &self,
        transaction: TransactionId,
    ) -> Option<String> {
        self.public
            .as_ref()?
            .output_candidate_cancellation_reason(transaction)
            .map(str::to_owned)
    }

    fn output_candidate_active(&self) -> bool {
        self.public
            .as_ref()
            .is_some_and(LivePublicPolicyState::output_candidate_active)
    }

    fn output_authority_topology_epoch(&self) -> Option<u64> {
        self.public
            .as_ref()
            .and_then(LivePublicPolicyState::output_authority_topology_epoch)
    }

    fn publish_output_authority_snapshot(
        &mut self,
        snapshot: sophia_protocol::OutputAuthoritySnapshot,
        capabilities: Vec<sophia_backend_live::LibdrmNativeOutputCapability>,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let Some(public) = self.public.as_mut() else {
            return Ok(false);
        };
        public.publish_output_authority_snapshot(snapshot, capabilities)
    }

    fn reject_output_topology_effect(
        &mut self,
        transaction: TransactionId,
        failure: sophia_engine::OutputTopologyTransactionFailure,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.public
            .as_mut()
            .ok_or("output effect observation requires the public policy owner")?
            .reject_output_topology_effect(transaction, failure)
    }

    fn begin_output_topology_apply(
        &mut self,
        transaction: TransactionId,
        heads: &[sophia_engine::RenderHeadId],
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.public
            .as_mut()
            .ok_or("output apply requires the public policy owner")?
            .begin_output_topology_apply(transaction, heads)
    }

    fn observe_output_topology_applied(
        &mut self,
        transaction: TransactionId,
        heads: &[sophia_engine::RenderHeadId],
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.public
            .as_mut()
            .ok_or("output apply requires the public policy owner")?
            .observe_output_topology_applied(transaction, heads)
    }

    fn observe_output_topology_first_presented(
        &mut self,
        transaction: TransactionId,
        outputs: &[sophia_protocol::OutputId],
    ) -> Result<Option<sophia_protocol::OutputAuthoritySnapshot>, Box<dyn std::error::Error>> {
        self.public
            .as_mut()
            .ok_or("output presentation requires the public policy owner")?
            .observe_output_topology_first_presented(transaction, outputs)
    }

    fn preview_output_topology_first_presented(
        &self,
        transaction: TransactionId,
        outputs: &[sophia_protocol::OutputId],
    ) -> Result<sophia_protocol::OutputAuthoritySnapshot, Box<dyn std::error::Error>> {
        self.public
            .as_ref()
            .ok_or("output presentation preview requires the public policy owner")?
            .preview_output_topology_first_presented(transaction, outputs)
    }

    fn observe_output_topology_rolled_back(
        &mut self,
        transaction: TransactionId,
        heads: &[sophia_engine::RenderHeadId],
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.public
            .as_mut()
            .ok_or("output rollback requires the public policy owner")?
            .observe_output_topology_rolled_back(transaction, heads)
    }

    fn observe_output_topology_rollback_failed(
        &mut self,
        transaction: TransactionId,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.public
            .as_mut()
            .ok_or("output rollback requires the public policy owner")?
            .observe_output_topology_rollback_failed(transaction)
    }
}

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

#[cfg(test)]
fn observe_public_output_topology(
    generations: &mut BTreeMap<sophia_protocol::OutputId, u64>,
    live: &mut BTreeSet<sophia_protocol::OutputId>,
    active: &mut sophia_protocol::OutputId,
    outputs: &[sophia_engine::HeadlessOutput],
) -> Result<bool, Box<dyn std::error::Error>> {
    let topology = output_topology_from_engine_outputs(outputs)?;
    let next = outputs.iter().map(|output| output.id).collect::<BTreeSet<_>>();
    let changed = next != *live;
    let mut candidate_generations = generations.clone();
    let mut candidate_live = live.clone();
    observe_public_output_generations(
        &mut candidate_generations,
        &mut candidate_live,
        outputs,
    )?;
    let candidate_active = if next.contains(active) {
        *active
    } else {
        topology.primary
    };
    *generations = candidate_generations;
    *live = candidate_live;
    *active = candidate_active;
    Ok(changed)
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
            prepared: None,
            shortcut_profile_slot,
            actions: Vec::new(),
            accepted_configuration: None,
            launch_classifications: BTreeMap::new(),
            launch_origins: Arc::new(Mutex::new(crate::launch_origin::LaunchOriginRegistry::default())),
            staged_launch_contexts: Vec::new(),
            in_flight_origin_surfaces: Vec::new(),
            outputs: outputs.to_vec(),
            output_bounds,
            output_generations,
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

    fn poll_public_request(
        &mut self,
        layout: &mut PersistentLiveLayout,
        _output: sophia_engine::HeadlessOutput,
        allow_new_cycle: bool,
    ) -> Result<Option<LiveWmProposal>, Box<dyn std::error::Error>> {
        if self.control_restart.is_some() || self.degraded { return Ok(None); }
        let chrome_style = self.candidate_chrome_style();
        let mut public = self.public.take().expect("public WM state is present");
        public.poll_output_authority()?;
        public.flush_deferred_command()?;
        let event = public
            .worker
            .as_ref()
            .ok_or("public WM transport is unavailable")?
            .try_event();
        let mut transport_failed = None;
        let mut defer_cycle = false;
        let proposal = match event {
            Ok(Some(PolicyTransportEvent::Negotiated)) => {
                public.negotiated = true;
                if let Some(key) = public.profile_key {
                    crate::session_println!("sophia_session_profile schema=1 status=activated role=wm epoch={} generation={} digest={}", public.connection_epoch, key.generation().raw(), key.digest());
                }
                // A client that negotiated is a client that works, so the
                // restart budget starts over. Without this the count only ever
                // rises: ProcessHealthy is the one event that clears it and
                // nothing emitted it, so three restarts across an entire
                // session -- however many hours apart, however deliberate --
                // ended the desktop on the third.
                let (state, _) = update_supervisor(
                    self.supervisor_state.clone(),
                    SupervisorEvent::ProcessHealthy,
                    self.restart_policy,
                );
                self.supervisor_state = state;
                None
            }
            Ok(Some(PolicyTransportEvent::ReadyForCycle { capabilities })) => {
                public.selected_capabilities = capabilities;
                if let Ok(mut origins) = public.launch_origins.lock() { origins.set_epoch(if capabilities & sophia_protocol::SOPHIA_WM_CAPABILITY_LAUNCH_ORIGIN != 0 { public.connection_epoch } else { 0 }); }
                public.transport_ready = true;
                None
            }
            Ok(Some(PolicyTransportEvent::Configuration {
                transaction,
                configuration,
            })) => {
                defer_cycle = true;
                let outcome = self.stage_policy_configuration(&mut public, &configuration)?;
                public.submit_or_defer(PolicyTransportCommand::ConfigurationOutcome {
                        transaction,
                        generation: configuration.generation,
                        outcome,
                    })?;
                if outcome != sophia_protocol::PolicyProjectionOutcome::Committed {
                    transport_failed = Some("invalid_configuration".to_owned());
                }
                None
            }
            Ok(Some(PolicyTransportEvent::Projection(projection))) => {
                let source = public
                    .in_flight_source
                    .ok_or("public WM projection has no owner cause")?;
                // Surface withdrawal may race a policy response. Advance the
                // canonical scene before touching response placements so a
                // proposal derived from the retired snapshot is rejected as
                // stale instead of trying to materialize a dead surface.
                let current_scene = public.snapshot(layout, chrome_style)?;
                if current_scene.generation > public.reducer.scene().generation {
                    public.reducer.observe_scene(current_scene)?;
                }
                if projection.base_generation != public.reducer.scene().generation {
                    defer_cycle = true;
                    public.settle_rejected_projection(
                        &projection,
                        sophia_protocol::PolicyProjectionOutcome::RejectedStale,
                    )?;
                    self.stale_responses = self.stale_responses.saturating_add(1);
                    crate::session_println!(
                        "sophia_live_wm schema=1 status=stale_response_rejected transaction={} reason=scene_advanced rearmed=true",
                        projection.transaction.raw(),
                    );
                    None
                } else {
                    if let LiveWmProposalSource::Manage(surface) = source {
                        layout.synchronize_admission_extent(surface);
                    }
                    let reconciliation = reconcile_public_policy_proposal(
                        layout,
                        &projection,
                        &public.work_areas,
                        &public.output_bounds,
                        chrome_style,
                    )?;
                    if reconciliation.adjusted_surfaces != 0 {
                        crate::session_println!(
                            "sophia_live_wm schema=1 status=constraints_reconciled transaction={} adjusted_surfaces={}",
                            reconciliation.policy.transaction.raw(),
                            reconciliation.adjusted_surfaces,
                        );
                    }
                    let context_valid = projection.launch_contexts.iter().all(|context| context.epoch == public.connection_epoch && public.reducer.scene().surfaces.iter().any(|s| s.surface == context.surface));
                    match if context_valid { public.reducer.stage_proposal(&reconciliation.policy) } else { Err(sophia_protocol::PolicyProjectionOutcome::RejectedInvalid) } {
                    Ok(staged) => {
                        let expected_operation_slot = match source {
                            LiveWmProposalSource::Action(action) => public
                                .actions
                                .iter()
                                .find(|registered| registered.action == action)
                                .and_then(|registered| registered.session_operation_slot),
                            _ => None,
                        };
                        let expect_session_operation = expected_operation_slot.is_some();
                        let identity = LivePolicySettlementIdentity {
                            connection_epoch: projection.connection_epoch,
                            request_id: projection.request_id,
                            scene_generation: projection.base_generation,
                            transaction: projection.transaction,
                            expect_session_operation,
                            session_operation: false,
                        };
                        public.expected_operation_slot = expected_operation_slot;
                        let projections = staged.projections();
                        let active_output = projection.active_output;
                        public.staged_launch_contexts = projection.launch_contexts.clone();
                        public.staged = Some(staged);
                        let mut live = public_live_proposal(
                            layout,
                            active_output,
                            projections,
                            projection.transaction,
                            source,
                            identity,
                            &reconciliation,
                        )?;
                        for layer in &mut live.layers {
                            if !projection.outputs.iter().any(|output| {
                                Some(output.output) == layer.output
                            }) {
                                continue;
                            }
                            layer.translation = projection.translation_groups.iter()
                                .find(|group| {
                                    Some(group.output) == layer.output
                                        && group.members.contains(&layer.surface)
                                })
                                .map(|group| sophia_protocol::LayerTranslation {
                                    connection_epoch: projection.connection_epoch,
                                    group: group.group,
                                    x: group.x,
                                    y: group.y,
                                });
                        }
                        Some(live)
                    }
                    Err(outcome) => {
                        defer_cycle = true;
                        public.settle_rejected_projection(&reconciliation.policy, outcome)?;
                        None
                    }
                    }
                }
            }
            Ok(Some(PolicyTransportEvent::Dirty(request))) => {
                if let Err(error) = public.admit_dirty(request) {
                    transport_failed = Some(format!("invalid_dirty:{error}"));
                }
                None
            }
            Ok(Some(PolicyTransportEvent::SessionOperation {
                transaction,
                request,
            })) => {
                let identity = LivePolicySettlementIdentity {
                    connection_epoch: request.connection_epoch,
                    request_id: request.request_id,
                    scene_generation: public.reducer.scene().generation,
                    transaction,
                    expect_session_operation: false,
                    session_operation: true,
                };
                let action = public.operation_actions.get(&request.operation).copied();
                let operation = public
                    .session_operations
                    .iter()
                    .find(|operation| operation.token == request.operation);
                let expected_slot = public.expected_operation_slot.take();
                let valid_target = request.target.is_none_or(|target| {
                    public
                        .reducer
                        .scene()
                        .surfaces
                        .iter()
                        .any(|surface| surface.surface == target)
                });
                let target_permitted = match (operation, request.target) {
                    (Some(operation), Some(_)) => operation.permits_surface_target,
                    (Some(_), None) => true,
                    (None, _) => false,
                };
                if request.connection_epoch != public.connection_epoch
                    || action.is_none()
                    || operation.map(|operation| operation.slot) != expected_slot
                    || !valid_target
                    || !target_permitted
                {
                    defer_cycle = true;
                    public.submit_or_defer(PolicyTransportCommand::SessionOperationOutcome {
                            transaction,
                            request_id: request.request_id,
                            outcome: sophia_protocol::PolicyProjectionOutcome::RejectedInvalid,
                        })?;
                    None
                } else {
                    public.pending_operation = Some((transaction, request));
                    Some(public_operation_proposal(
                        layout,
                        transaction,
                        identity,
                    ))
                }
            }
            Ok(Some(PolicyTransportEvent::Failed(error))) => {
                transport_failed = Some(error);
                None
            }
            Ok(None) => None,
            Err(()) => {
                transport_failed = Some("worker_disconnected".to_owned());
                None
            }
        };

        public.materialize_pending_dirty();

        if proposal.is_none()
            && transport_failed.is_none()
            && !defer_cycle
            && allow_new_cycle
            && public.configured
            && !public.cycle_submitted
            && public.transport_ready
            && public.in_flight_request.is_none()
            && public.deferred_command.is_none()
            && !public.queue.is_empty()
        {
            let scene = public.snapshot(layout, chrome_style)?;
            if scene.generation > public.reducer.scene().generation {
                public.reducer.observe_scene(scene.clone())?;
            }
            // A cause whose subject is gone is moot, and the projection
            // reducer refuses it outright rather than ignoring it, which ends
            // the session. Withdrawal raises its own cause, so dropping this
            // one loses nothing. Causes are only queued long enough to matter
            // because ordinary cycles are held for the whole of a topology
            // candidate, which is exactly when a surface can disappear.
            let mut dropped = 0usize;
            let cause = loop {
                let Some(cause) = public.queue.pop_front() else {
                    break None;
                };
                if let sophia_protocol::PolicyRequestCause::Action { activation_serial, .. } = cause.cause
                    && let Some(ticket) = public.control_tickets.get(&activation_serial)
                {
                    if ticket.cancelled() || ticket.generation != public.control_generation {
                        ticket.finish(sophia_protocol::ControlOutcome::Stale);
                        public.control_tickets.remove(&activation_serial);
                        continue;
                    }
                    if !ticket.claim() {
                        public.queue.push_front(cause);
                        break None;
                    }
                }
                if policy_cause_subject_is_live(cause.cause, &scene) {
                    break Some(cause);
                }
                dropped = dropped.saturating_add(1);
            };
            if dropped != 0 {
                tracing::warn!(
                    "sophia_live_wm_policy schema=1 status=cause_withdrawn dropped={dropped}",
                );
            }
            let Some(cause) = cause else {
                self.public = Some(public);
                return Ok(proposal);
            };
            // A cause names the outputs it was raised for, and it may have been
            // queued before a topology change replaced them. Its outputs are a
            // hint about where work is owed, not an identity, so they are
            // resolved against the scene the request will actually carry. A
            // cause that outlived every output it named still needs servicing:
            // the topology moved, which is precisely a reason to lay out again.
            let affected_outputs = resolve_public_policy_affected_outputs(
                cause.affected_outputs,
                scene.outputs.iter().map(|output| output.output),
            );
            let request = public
                .reducer
                .issue_request_with_cause(affected_outputs, cause.cause)?;
            let snapshot_transaction = public.mint_transaction()?;
            let request_transaction = public.mint_transaction()?;
            let classifications =
                public_launch_classification_snapshot(&public.launch_classifications, &scene);
            let launch_origins = public.launch_origins.lock().map(|r| r.origins(scene.surfaces.iter().map(|s| s.surface))).unwrap_or_default();
            public.in_flight_origin_surfaces = launch_origins.iter().map(|c| c.surface).collect();
            public
                .worker
                .as_ref()
                .ok_or("public WM transport is unavailable")?
                .try_command(PolicyTransportCommand::Cycle {
                    snapshot_transaction,
                    request_transaction,
                    scene: Box::new(scene),
                    actions: public.actions.clone(),
                    classifications,
                    launch_origins,
                    request: request.clone(),
                })
                .map_err(|_| "public WM cycle queue is busy")?;
            public.in_flight_source = Some(cause.source);
            public.in_flight_request = Some(request);
            public.cycle_submitted = true;
            public.transport_ready = false;
            self.requests = self.requests.saturating_add(1);
        }
        self.public = Some(public);
        if let Some(error) = transport_failed {
            self.request_transport_restart("public_transport_failed", Some(&error));
        }
        Ok(proposal)
    }

    fn poll_public_restart(
        &mut self,
        layout: &mut PersistentLiveLayout,
        output: sophia_engine::HeadlessOutput,
    ) -> Result<Option<LiveWmProposal>, Box<dyn std::error::Error>> {
        if self.control_restart.is_some() {
            self.poll_control_restart(layout, output);
            return Ok(None);
        }
        if self.degraded {
            return Ok(None);
        }
        self.poll_public_proof_restart()?;
        let restart_requested = self.force_transport_restart;
        let process_exited = self.supervisor.poll()?.is_some();
        let settlement_pending = public_policy_restart_settlement_pending(
            layout
                .pending
                .as_ref()
                .is_some_and(|pending| pending.policy_settlement.is_some()),
            self.public
                .as_ref()
                .is_some_and(|public| public.output_effect_dispatched),
        );
        match public_policy_restart_decision(
            restart_requested,
            process_exited,
            settlement_pending,
        ) {
            PublicPolicyRestartDecision::Idle => return Ok(None),
            PublicPolicyRestartDecision::AbortSettlement => {
                if !process_exited {
                    self.supervisor.terminate()?;
                }
                let public = self.public.as_mut().expect("public WM state is present");
                public.request_output_candidate_cancellation(
                    "supervised WM restart requested during output apply".to_owned(),
                    None,
                )?;
                public.worker.take();
                public.transport_unavailable = true;
                public.deferred_command = None;
                self.force_transport_restart = true;
                layout.force_pending_timeout();
                crate::session_println!(
                    "sophia_live_wm schema=4 status=settlement_aborting adapter=sophia_wm_v1 reason=transport_lost preserved_layout=true"
                );
                return Ok(None);
            }
            PublicPolicyRestartDecision::Restart => {}
        }
        if restart_requested && !process_exited {
            self.supervisor.terminate()?;
        }
        if self.desktop_reload.as_ref().is_some_and(|pending| pending.replacement_spec.is_none()) {
            self.pending_policy_launch_spec = self.rollback_desktop_reload();
        }
        let replacement = self.pending_policy_launch_spec.take().or_else(|| {
            self.desktop_reload.as_mut().and_then(|pending| pending.replacement_spec.take())
        });
        if let Some(spec) = replacement {
            self.supervisor = ProcessSupervisor::new(SupervisedProcessKind::WindowManager, spec);
        }
        let mut public = self.public.take().expect("public WM state is present");
        public.worker.take();
        self.pending_policy_configuration = None;
        self.control_lifetime.take();
        for (_, ticket) in std::mem::take(&mut public.control_tickets) {
            ticket.finish(if ticket.dispatched() { sophia_protocol::ControlOutcome::Indeterminate } else { sophia_protocol::ControlOutcome::Stale });
        }
        let _ = public.reducer.disconnect(public.connection_epoch);
        // Retain the key ledger until replacement acceptance; never synthesize releases.

        self.force_transport_restart = false;
        self.restarts = self.restarts.saturating_add(1);
        if let Some(output_service) = public.output_service.as_ref() {
            let abandoned = output_service
                .pause_acceptance(Duration::from_secs(1))
                .map_err(|error| format!("output authority restart barrier failed: {error}"))?;
            if !abandoned.is_empty() {
                public.abandon_output_candidate()?;
            }
            crate::session_println!(
                "sophia_live_output_authority schema=2 status=acceptance_paused abandoned={} preserved_topology=true",
                abandoned.len(),
            );
        }
        let next_epoch = public.next_connection_epoch;
        public.next_connection_epoch = public
            .next_connection_epoch
            .checked_add(1)
            .ok_or("public WM connection epoch exhausted")?;
        let mut transport = bind_public_policy_transport(&public.directory, public.profile_key)?;
        let (state, command) = update_supervisor(
            self.supervisor_state.clone(),
            SupervisorEvent::ProcessExited,
            self.restart_policy,
        );
        self.supervisor_state = state;
        let started = match self.supervisor.apply(command) {
            Ok(Some(started)) => started,
            Ok(None) => return Err("public WM supervisor did not restart the policy process".into()),
            Err(error) => {
                if self.desktop_reload.is_some() {
                    self.public = Some(public);
                    self.pending_policy_launch_spec = self.rollback_desktop_reload();
                    self.force_transport_restart = true;
                    return Ok(None);
                }
                if self.committed == 0 {
                    return Err(error.into());
                }
                self.degraded = true;
                self.public = Some(public);
                crate::session_println!(
                    "sophia_live_wm schema=4 status=degraded adapter=sophia_wm_v1 reason=restart_failed preserved_layout=true error={error:?}"
                );
                return Ok(None);
            }
        };
        let pid = self
            .supervisor
            .peer_id()
            .ok_or("restarted public WM has no supervised PID")?;
        transport.authorize_supervised_pid(pid)?;
        crate::diagnostics::capture_process_identity("wm", pid, next_epoch);
        if let Some(output_service) = public.output_service.as_ref() {
            output_service
                .command(
                    sophia_runtime::OutputTransportServiceCommand::ReplaceSupervisedPid { pid },
                )
                .map_err(|_| "output authority service is unavailable during WM restart")?;
        }
        let (state, _) = update_supervisor(self.supervisor_state.clone(), started, self.restart_policy);
        self.supervisor_state = state;
        public.reducer.connect(next_epoch)?;
        public.worker = Some(start_public_policy_worker(
            transport,
            next_epoch,
            public.profile_key,
        )?);
        public.connection_epoch = next_epoch;
        public.configured = false;
        public.negotiated = false;
        public.cycle_submitted = false;
        public.transport_ready = false;
        public.in_flight_request = None;
        public.in_flight_source = None;
        public.staged = None;
        public.prepared = None;
        public.pending_operation = None;
        public.expected_operation_slot = None;
        public.deferred_command = None;
        public.transport_unavailable = false;
        public.actions.clear();
        public.queue.clear();
        public.pending_dirty_outputs.clear();
        // The restarted policy has answered nothing. Its predecessor's settled
        // answers do not bind it, and the epoch key alone would not release them
        // until the first commit of the new connection.
        layout.rearm_manage_settlements();
        let affected_outputs = public.all_outputs(output.id);
        public.queue.push_back(LivePublicPolicyCause {
            source: LiveWmProposalSource::Relayout,
            cause: sophia_protocol::PolicyRequestCause::SceneChanged,
            affected_outputs,
        });
        self.public = Some(public);
        crate::session_println!(
            "sophia_live_wm schema=4 status=restarted adapter=sophia_wm_v1 epoch={next_epoch} restarts={} preserved_layout=true",
            self.restarts
        );
        Ok(None)
    }

    fn update_public_work_areas_at(
        &mut self,
        layout: &PersistentLiveLayout,
        outputs: &[sophia_engine::HeadlessOutput],
        full_bounds: &[(sophia_protocol::OutputId, Rect)],
        primary: sophia_engine::HeadlessOutput,
    ) -> Result<LiveWmRequestAdmission, Box<dyn std::error::Error>> {
        let root = full_bounds.iter().try_fold(
            Rect {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            },
            |root, (_, bounds)| {
                Some(Rect {
                    x: 0,
                    y: 0,
                    width: root.width.max(bounds.x.checked_add(bounds.width)?),
                    height: root.height.max(bounds.y.checked_add(bounds.height)?),
                })
            },
        );
        let Some(root) = root.filter(|root| !root.is_empty()) else {
            return Err("public WM output topology has no valid root bounds".into());
        };
        let reduced = sophia_engine::reduce_output_work_areas(
            root,
            full_bounds.iter().copied(),
            &layout.active_output_reservations(),
            &self.shell_reservation_bands,
        );
        let chrome_style = self.candidate_chrome_style();
        let public = self.public.as_mut().expect("public WM state is present");
        let next_live = outputs
            .iter()
            .map(|output| output.id)
            .collect::<BTreeSet<_>>();
        let mut next_generations = public.output_generations.clone();
        let mut generation_live = public.live_output_ids.clone();
        observe_public_output_generations(
            &mut next_generations,
            &mut generation_live,
            outputs,
        )?;
        if generation_live != next_live {
            return Err("public WM output-generation projection is incomplete".into());
        }
        let next_active = if next_live.contains(&public.active_output) {
            public.active_output
        } else {
            primary.id
        };
        let next_bounds = full_bounds.iter().copied().collect::<BTreeMap<_, _>>();
        let mut next_work_areas = public.work_areas.clone();
        next_work_areas.retain(|output, _| next_live.contains(output));
        for area in reduced {
            let Some(work) = area.work else {
                continue;
            };
            next_work_areas.insert(area.output, work);
        }
        let changed = public.outputs != outputs
            || public.live_output_ids != next_live
            || public.output_bounds != next_bounds
            || public.work_areas != next_work_areas
            || public.active_output != next_active;
        if !changed {
            return Ok(LiveWmRequestAdmission::Duplicate);
        }
        let mut affected_outputs = next_live.iter().copied().collect::<Vec<_>>();
        if let Some(index) = affected_outputs
            .iter()
            .position(|output| *output == primary.id)
        {
            affected_outputs.swap(0, index);
        }
        let cause = LivePublicPolicyCause {
            source: LiveWmProposalSource::Relayout,
            cause: sophia_protocol::PolicyRequestCause::SceneChanged,
            affected_outputs,
        };
        let mut next_queue = public.queue.clone();
        let admission = enqueue_public_policy_cause(
            &mut next_queue,
            public.in_flight_source,
            public.in_flight_request.is_some(),
            cause,
        );
        if admission == LiveWmRequestAdmission::RejectedCapacity {
            return Ok(admission);
        }
        if admission == LiveWmRequestAdmission::Duplicate {
            // A queued relayout still names the previous live set. Dropping the
            // replacement would leave a cause pointing at an output that no
            // longer exists, and issuing that cause fails the session. Merge
            // instead, the same way owner-observed dirty outputs fold in.
            let mut dirty = next_live.iter().copied().collect::<BTreeSet<_>>();
            materialize_public_dirty_cause(&mut next_queue, &mut dirty, public.in_flight_source);
        }
        public.outputs = outputs.to_vec();
        public.output_generations = next_generations;
        public.live_output_ids = next_live;
        public.output_bounds = next_bounds;
        for (output, work) in &next_work_areas {
            if public.work_areas.get(output) != Some(work) {
                crate::session_println!(
                    "sophia_live_work_area schema=1 output={} x={} y={} width={} height={} app_reservations={} shell_reservations={}",
                    output.raw(), work.x, work.y, work.width, work.height,
                    layout.active_output_reservations().len(), self.shell_reservation_bands.len(),
                );
            }
        }
        public.work_areas = next_work_areas;
        public.active_output = next_active;
        public.queue = next_queue;
        // Advance the reducer scene at the owner-observation boundary, before
        // a replacement request is issued. An in-flight response derived from
        // the retired output set is stale as soon as the owner accepts the new
        // topology; waiting until the next cycle would leave a click-through
        // window in which that response could still stage.
        let scene = public.snapshot(layout, chrome_style)?;
        if scene.generation > public.reducer.scene().generation {
            public.reducer.observe_scene(scene)?;
        }
        Ok(admission)
    }

    fn prepare_public_layout_commit(
        &mut self,
        layout: &PersistentLiveLayout,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let Some(identity) = layout
            .pending
            .as_ref()
            .and_then(|pending| pending.policy_settlement)
        else {
            return Ok(true);
        };
        if identity.session_operation {
            return Ok(true);
        }
        let public = self.public.as_mut().ok_or("public settlement lost its session")?;
        if public.prepared == Some(identity) {
            return Ok(true);
        }
        let staged = public
            .staged
            .as_ref()
            .ok_or("ready public layout lost its staged reducer successor")?;
        let outcome = public.reducer.revalidate_staged(staged);
        if outcome == sophia_protocol::PolicyProjectionOutcome::RejectedStale {
            return Ok(false);
        }
        if outcome != sophia_protocol::PolicyProjectionOutcome::Committed {
            return Err(format!(
                "ready public layout failed canonical revalidation: {outcome:?}"
            )
            .into());
        }
        public.prepared = Some(identity);
        crate::session_println!(
            "sophia_live_wm_chrome schema=2 status=acknowledged transaction={} request_id={} scene_generation={}",
            identity.transaction.raw(),
            identity.request_id,
            identity.scene_generation,
        );
        Ok(true)
    }

    fn trigger_public_proof_fault(&mut self, point: PublicPolicyFaultPoint) -> bool {
        let trigger = self.public.as_mut().is_some_and(|public| {
            if public.proof_fault_triggered || public.proof_fault_after != Some(point) {
                return false;
            }
            public.proof_fault_triggered = true;
            true
        });
        if trigger {
            self.request_transport_restart("public_policy_proof_fault", Some(point.name()));
            crate::session_println!(
                "sophia_live_wm schema=4 status=proof_fault_triggered adapter=sophia_wm_v1 phase={} preserved_layout=true",
                point.name(),
            );
        }
        trigger
    }

    fn poll_public_proof_restart(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let Some(public) = self.public.as_mut() else {
            return Ok(());
        };
        let Some(before) = public.proof_restart_checkpoint_before else {
            return Ok(());
        };
        let current = policy_checkpoint_identity(&public.checkpoint_path)?;
        if !policy_checkpoint_replaced(before, current) {
            return Ok(());
        }
        let action = public
            .proof_restart_after_action
            .expect("an armed checkpoint restart has an action");
        public.proof_restart_checkpoint_before = None;
        public.proof_restart_triggered = true;
        self.request_transport_restart("public_policy_checkpoint_proof", None);
        crate::session_println!(
            "sophia_live_wm schema=4 status=proof_restart_triggered adapter=sophia_wm_v1 phase=checkpoint_saved action={} preserved_layout=true",
            action.raw(),
        );
        Ok(())
    }

    fn public_settlement_abort_required(&self) -> bool {
        self.public
            .as_ref()
            .is_some_and(|public| public.transport_unavailable)
    }
}

include!("public_policy/proposal.rs");

include!("profile_reload.rs");
