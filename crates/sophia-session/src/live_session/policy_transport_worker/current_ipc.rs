//! Current public IPC adapter. No wire transfers escape into the driver.
use super::adapter::{PolicyAdapter, PolicyAdapterEvent, PolicyProfileAdmission};
use super::driver::PolicyReceivePermit;
use super::*;
use sophia_protocol::{
    WmV1ProfileIdentity, decode_wm_v1_policy_projection, encode_wm_v1_policy_snapshot,
};
use sophia_runtime::{PolicyClientEvent, PolicyWmSessionTransport, QueuedPolicyProjection};

struct CurrentPolicyIpc {
    transport: PolicyWmSessionTransport,
    connection_epoch: u64,
}

// Keep existing callers and supervised endpoint construction unchanged. New
// adapters enter at spawn, not through this IPC compatibility constructor.
impl PolicyTransportWorker {
    pub(in crate::live_session) fn new(
        transport: PolicyWmSessionTransport,
        connection_epoch: u64,
    ) -> Result<Self, std::io::Error> {
        Self::spawn(
            CurrentPolicyIpc {
                transport,
                connection_epoch,
            },
            connection_epoch,
            None,
        )
    }

    pub(in crate::live_session) fn new_profile_activated(
        transport: PolicyWmSessionTransport,
        connection_epoch: u64,
        identity: WmV1ProfileIdentity,
        prepare_transaction: TransactionId,
        activate_transaction: TransactionId,
    ) -> Result<Self, std::io::Error> {
        Self::spawn(
            CurrentPolicyIpc {
                transport,
                connection_epoch,
            },
            connection_epoch,
            Some(PolicyProfileAdmission {
                connection_epoch: identity.connection_epoch,
                generation: identity.profile_generation,
                digest: identity.profile_digest,
                prepare_transaction,
                activate_transaction,
            }),
        )
    }
}

fn decode_event(event: PolicyClientEvent) -> PolicyAdapterEvent {
    match event {
        PolicyClientEvent::ProjectionPending => PolicyAdapterEvent::ProjectionPending,
        PolicyClientEvent::Projection(QueuedPolicyProjection::Admitted(projection)) => {
            match decode_wm_v1_policy_projection(&projection.into_wire_transfer()) {
                Ok(proposal) => PolicyAdapterEvent::Projection(Box::new(proposal)),
                Err(error) => PolicyAdapterEvent::MalformedProjection(format!(
                    "policy projection decode failed: {error:?}"
                )),
            }
        }
        PolicyClientEvent::Projection(QueuedPolicyProjection::Discarded { .. }) => {
            PolicyAdapterEvent::ProjectionDiscarded
        }
        PolicyClientEvent::Configuration {
            transaction,
            configuration,
        } => PolicyAdapterEvent::Configuration {
            transaction,
            configuration,
        },
        PolicyClientEvent::Dirty { request, .. } => PolicyAdapterEvent::Dirty(request),
        PolicyClientEvent::SessionOperation {
            transaction,
            request,
        } => PolicyAdapterEvent::SessionOperation {
            transaction,
            request,
        },
        PolicyClientEvent::ProfileCompletion { .. } => {
            PolicyAdapterEvent::UnexpectedProfileCompletion
        }
    }
}

impl PolicyAdapter for CurrentPolicyIpc {
    fn admit(
        &mut self,
        connection_epoch: u64,
        profile: Option<PolicyProfileAdmission>,
    ) -> Result<(), String> {
        self.transport
            .accept_and_negotiate(connection_epoch, Duration::from_secs(4))
            .map_err(|error| error.to_string())?;
        if let Some(profile) = profile {
            // The caller already supplied this validated value. Reconstruct
            // the same passive wire identity without changing validation order.
            self.transport
                .activate_profile_handoff(
                    WmV1ProfileIdentity {
                        connection_epoch: profile.connection_epoch,
                        profile_generation: profile.generation,
                        profile_digest: profile.digest,
                    },
                    profile.prepare_transaction,
                    profile.activate_transaction,
                )
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    fn selected_capabilities(&self) -> u64 {
        self.transport.selected_capabilities()
    }

    fn receive_within(
        &mut self,
        _permit: PolicyReceivePermit,
        timeout: Duration,
    ) -> Result<PolicyAdapterEvent, String> {
        self.transport
            .receive_client_event_within(timeout)
            .map(decode_event)
            .map_err(|error| error.to_string())
    }

    fn try_receive(
        &mut self,
        _permit: PolicyReceivePermit,
    ) -> Result<Option<PolicyAdapterEvent>, String> {
        self.transport
            .try_receive_client_event()
            .map(|event| event.map(decode_event))
            .map_err(|error| error.to_string())
    }

    fn send(&mut self, command: &PolicyTransportCommand) -> Result<(), String> {
        let result = match command {
            PolicyTransportCommand::ConfigurationOutcome {
                transaction,
                generation,
                outcome,
            } => self
                .transport
                .send_configuration_outcome(*transaction, *generation, *outcome),
            PolicyTransportCommand::Cycle {
                snapshot_transaction,
                request_transaction,
                scene,
                actions,
                classifications,
                launch_origins,
                request,
            } => {
                let mut snapshot = encode_wm_v1_policy_snapshot(
                    *snapshot_transaction,
                    self.connection_epoch,
                    scene,
                    actions,
                    classifications,
                    self.transport.selected_capabilities(),
                )
                .map_err(|error| format!("policy snapshot encode failed: {error:?}"))?;
                sophia_protocol::append_wm_launch_origins(
                    &mut snapshot,
                    launch_origins,
                    self.transport.selected_capabilities(),
                )
                .map_err(|error| format!("launch origin encode: {error:?}"))?;
                self.transport
                    .send_snapshot(
                        snapshot.transaction,
                        &snapshot.begin,
                        &snapshot.chunks,
                        &snapshot.end,
                    )
                    .map_err(|error| error.to_string())?;
                self.transport
                    .send_projection_request(*request_transaction, request)
            }
            PolicyTransportCommand::ProjectionOutcome {
                transaction,
                request_id,
                scene_generation,
                outcome,
                ..
            } => self.transport.send_projection_outcome(
                *transaction,
                *request_id,
                *scene_generation,
                *outcome,
            ),
            PolicyTransportCommand::SessionOperationOutcome {
                transaction,
                request_id,
                outcome,
            } => self
                .transport
                .send_session_operation_outcome(*transaction, *request_id, *outcome),
            PolicyTransportCommand::PresentationReceipt {
                transaction,
                receipt,
            } => self
                .transport
                .send_presentation_receipt(*transaction, *receipt),
            PolicyTransportCommand::Stop => unreachable!("the driver handles stop"),
        };
        result.map_err(|error| error.to_string())
    }

    fn disconnect(&mut self) {
        let _ = self.transport.disconnect();
    }
}
