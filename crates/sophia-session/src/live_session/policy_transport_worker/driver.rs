use super::adapter::{PolicyAdapter, PolicyAdapterEvent, PolicyProfileAdmission};
use super::*;

/// Preserve the current owner phase discipline independently of byte framing.
pub(super) fn run_policy_transport(
    transport: &mut impl PolicyAdapter,
    connection_epoch: u64,
    profile_admission: Option<PolicyProfileAdmission>,
    commands: &Receiver<PolicyTransportCommand>,
    events: &SyncSender<PolicyTransportEvent>,
) -> Result<(), String> {
    transport.admit(connection_epoch, profile_admission)?;
    events
        .send(PolicyTransportEvent::Negotiated)
        .map_err(|_| "policy owner event channel disconnected".to_owned())?;

    let configuration = transport.receive_within(POLICY_CLIENT_RESPONSE_DEADLINE)?;
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

    loop {
        let command = match commands.recv_timeout(Duration::from_millis(10)) {
            Ok(command) => command,
            Err(RecvTimeoutError::Timeout) => {
                match transport.try_receive()? {
                    Some(PolicyAdapterEvent::Dirty(request)) => events
                        .send(PolicyTransportEvent::Dirty(request))
                        .map_err(|_| "policy owner event channel disconnected".to_owned())?,
                    Some(_) => {
                        return Err("policy client sent an out-of-phase control message".to_owned());
                    }
                    None => {}
                }
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => {
                return Err("policy owner command channel disconnected".to_owned());
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
                        match transport.receive_within(POLICY_CLIENT_RESPONSE_DEADLINE)? {
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
                    let event = transport.receive_within(POLICY_CLIENT_RESPONSE_DEADLINE)?;
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
