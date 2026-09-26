use super::adapter::{PolicyAdapter, PolicyAdapterEvent, PolicyProfileAdmission};
use super::*;

// Liveness bound only. File traffic and commands are serviced by readiness,
// not by a latency timer. Staging expiry can shorten the reactor's wait.
const POLICY_FILE_IDLE_LIVENESS_CAP: Duration = POLICY_CLIENT_RESPONSE_DEADLINE;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PolicyReceiveKind {
    Negotiate,
    ProfileCompletion(sophia_runtime::PolicyProfileHandoffEffect),
    Configuration,
    DirtyOnly,
    Projection { allow_dirty: bool },
    SessionOperation,
}

/// One driver invocation grants one offer receive. Profile permissions can
/// subsequently be derived only from the existing reducer's exact Send effect.
pub(super) struct PolicyAdmissionPermit(());
pub(super) struct PolicyProfilePermit(());
impl PolicyAdmissionPermit {
    pub(super) fn negotiate(self) -> (PolicyReceivePermit, PolicyProfilePermit) {
        (
            PolicyReceivePermit {
                kind: PolicyReceiveKind::Negotiate,
            },
            PolicyProfilePermit(()),
        )
    }
}
impl PolicyProfilePermit {
    pub(super) fn completion(
        &self,
        effect: sophia_runtime::PolicyProfileHandoffEffect,
    ) -> PolicyReceivePermit {
        PolicyReceivePermit {
            kind: PolicyReceiveKind::ProfileCompletion(effect),
        }
    }
}

/// Issued only at the driver's existing wait sites. Not Clone/Copy: a file
/// owner may transfer one complete candidate under it, then must return to
/// the driver for another. File fragments never advance this permission.
#[cfg_attr(not(test), allow(dead_code))] // Consumed by the file adapter in the next checkpoint.
pub(super) struct PolicyReceivePermit {
    kind: PolicyReceiveKind,
}

#[cfg_attr(not(test), allow(dead_code))] // Current IPC deliberately ignores admission hints.
impl PolicyReceivePermit {
    pub(super) fn kind(&self) -> PolicyReceiveKind {
        self.kind
    }

    pub(super) fn allows(&self, event: &PolicyAdapterEvent) -> bool {
        if let (
            PolicyReceiveKind::ProfileCompletion(effect),
            PolicyAdapterEvent::ProfileCompletion { kind, .. },
        ) = (self.kind, event)
        {
            // The shared reducer retains exact identity/outcome correlation.
            return effect.kind == *kind;
        }
        matches!(
            (self.kind, event),
            (
                PolicyReceiveKind::Negotiate,
                PolicyAdapterEvent::Negotiation(_)
            ) | (
                PolicyReceiveKind::Configuration,
                PolicyAdapterEvent::Configuration { .. }
            ) | (PolicyReceiveKind::DirtyOnly, PolicyAdapterEvent::Dirty(_))
                | (
                    PolicyReceiveKind::Projection { .. },
                    PolicyAdapterEvent::Projection(_)
                )
                | (
                    PolicyReceiveKind::Projection { allow_dirty: true },
                    PolicyAdapterEvent::Dirty(_)
                )
                | (
                    PolicyReceiveKind::SessionOperation,
                    PolicyAdapterEvent::SessionOperation { .. }
                )
        )
    }
}

/// Preserve the current owner phase discipline independently of byte framing.
pub(super) fn run_policy_transport(
    transport: &mut impl PolicyAdapter,
    connection_epoch: u64,
    profile_admission: Option<PolicyProfileAdmission>,
    commands: &Receiver<PolicyTransportCommand>,
    events: &SyncSender<PolicyTransportEvent>,
) -> Result<(), String> {
    transport.admit(
        PolicyAdmissionPermit(()),
        connection_epoch,
        profile_admission,
    )?;
    events
        .send(PolicyTransportEvent::Negotiated)
        .map_err(|_| "policy owner event channel disconnected".to_owned())?;

    let configuration = transport.receive_within(
        PolicyReceivePermit {
            kind: PolicyReceiveKind::Configuration,
        },
        POLICY_CLIENT_RESPONSE_DEADLINE,
    )?;
    let PolicyAdapterEvent::Configuration {
        transaction,
        configuration,
    } = configuration
    else {
        return Err("policy client did not configure before its first snapshot".to_owned());
    };
    events
        .send(PolicyTransportEvent::Configuration {
            transaction,
            configuration,
        })
        .map_err(|_| "policy owner event channel disconnected".to_owned())?;

    let readiness_idle = transport.command_wake_handle().is_some();
    loop {
        let command = if readiness_idle {
            match commands.try_recv() {
                Ok(command) => command,
                Err(TryRecvError::Disconnected) => {
                    return Err("policy owner command channel disconnected".to_owned());
                }
                Err(TryRecvError::Empty) => {
                    match transport.idle_receive(
                        PolicyReceivePermit {
                            kind: PolicyReceiveKind::DirtyOnly,
                        },
                        POLICY_FILE_IDLE_LIVENESS_CAP,
                    )? {
                        Some(PolicyAdapterEvent::Dirty(request)) => events
                            .send(PolicyTransportEvent::Dirty(request))
                            .map_err(|_| "policy owner event channel disconnected".to_owned())?,
                        Some(_) => {
                            return Err(
                                "policy client sent an out-of-phase control message".to_owned()
                            );
                        }
                        None => {}
                    }
                    continue;
                }
            }
        } else {
            match commands.recv_timeout(Duration::from_millis(10)) {
                Ok(command) => command,
                Err(RecvTimeoutError::Timeout) => {
                    match transport.try_receive(PolicyReceivePermit {
                        kind: PolicyReceiveKind::DirtyOnly,
                    })? {
                        Some(PolicyAdapterEvent::Dirty(request)) => events
                            .send(PolicyTransportEvent::Dirty(request))
                            .map_err(|_| "policy owner event channel disconnected".to_owned())?,
                        Some(_) => {
                            return Err(
                                "policy client sent an out-of-phase control message".to_owned()
                            );
                        }
                        None => {}
                    }
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return Err("policy owner command channel disconnected".to_owned());
                }
            }
        };
        if matches!(command, PolicyTransportCommand::Stop) {
            return Ok(());
        }
        transport.send(&command)?;
        match command {
            PolicyTransportCommand::ConfigurationOutcome { outcome, .. } => {
                if outcome == PolicyProjectionOutcome::Committed {
                    events
                        .send(PolicyTransportEvent::ReadyForCycle {
                            capabilities: transport.selected_capabilities(),
                        })
                        .map_err(|_| "policy owner event channel disconnected".to_owned())?;
                }
            }
            PolicyTransportCommand::Cycle { .. } => {
                let mut projection_started = false;
                let proposal =
                    loop {
                        match transport.receive_within(
                            PolicyReceivePermit {
                                kind: PolicyReceiveKind::Projection {
                                    allow_dirty: !projection_started,
                                },
                            },
                            POLICY_CLIENT_RESPONSE_DEADLINE,
                        )? {
                            PolicyAdapterEvent::ProjectionPending => projection_started = true,
                            PolicyAdapterEvent::Projection(projection) => break projection,
                            PolicyAdapterEvent::MalformedProjection(error) => return Err(error),
                            PolicyAdapterEvent::Dirty(request) if !projection_started => {
                                events.send(PolicyTransportEvent::Dirty(request)).map_err(
                                    |_| "policy owner event channel disconnected".to_owned(),
                                )?;
                            }
                            PolicyAdapterEvent::ProjectionDiscarded => {
                                return Err("policy projection transfer was discarded".to_owned());
                            }
                            _ => return Err(
                                "policy client sent a control message during projection transfer"
                                    .to_owned(),
                            ),
                        }
                    };
                if !proposal.output_launch_contexts.is_empty()
                    && (transport.selected_capabilities()
                        & sophia_protocol::SOPHIA_WM_CAPABILITY_OUTPUT_LAUNCH_CONTEXT
                        == 0
                        || transport.selected_capabilities()
                            & sophia_protocol::SOPHIA_WM_CAPABILITY_LAUNCH_ORIGIN
                            == 0)
                {
                    return Err("unnegotiated output launch context".to_owned());
                }
                if !proposal.launch_contexts.is_empty()
                    && transport.selected_capabilities()
                        & sophia_protocol::SOPHIA_WM_CAPABILITY_LAUNCH_ORIGIN
                        == 0
                {
                    return Err("unnegotiated launch context".to_owned());
                }
                events
                    .send(PolicyTransportEvent::Projection(proposal))
                    .map_err(|_| "policy owner event channel disconnected".to_owned())?;
            }
            PolicyTransportCommand::ProjectionOutcome {
                outcome,
                expect_session_operation,
                ..
            } => {
                if expect_session_operation && outcome == PolicyProjectionOutcome::Committed {
                    let event = transport.receive_within(
                        PolicyReceivePermit {
                            kind: PolicyReceiveKind::SessionOperation,
                        },
                        POLICY_CLIENT_RESPONSE_DEADLINE,
                    )?;
                    let PolicyAdapterEvent::SessionOperation {
                        transaction,
                        request,
                    } = event
                    else {
                        return Err(
                            "policy client omitted its committed session operation".to_owned()
                        );
                    };
                    events
                        .send(PolicyTransportEvent::SessionOperation {
                            transaction,
                            request,
                        })
                        .map_err(|_| "policy owner event channel disconnected".to_owned())?;
                } else {
                    events
                        .send(PolicyTransportEvent::ReadyForCycle {
                            capabilities: transport.selected_capabilities(),
                        })
                        .map_err(|_| "policy owner event channel disconnected".to_owned())?;
                }
            }
            PolicyTransportCommand::PresentationReceipt { .. } => {}
            PolicyTransportCommand::SessionOperationOutcome { .. } => {
                events
                    .send(PolicyTransportEvent::ReadyForCycle {
                        capabilities: transport.selected_capabilities(),
                    })
                    .map_err(|_| "policy owner event channel disconnected".to_owned())?;
            }
            PolicyTransportCommand::Stop => unreachable!("stop is never sent to an adapter"),
        }
    }
}
