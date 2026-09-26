//! Complete private adapter on an already admitted stream. No launch selection
//! or semantic settlement lives here; the existing driver owns command order.
use super::super::PolicyTransportCommand;
use super::super::adapter::{PolicyAdapter, PolicyProfileAdmission};
use super::super::driver::PolicyAdmissionPermit;
use super::startup::FileStartup;
use super::typed_codec::{TypedFileCodec, codec_error};
use super::*;
use sophia_protocol::{PolicyDecodedSnapshot, PolicySessionOperationOutcome};

#[path = "../../../../tests/support/policy_file_adapter.rs"]
mod tests;

#[path = "../../../../tests/support/policy_file_nim_peer.rs"]
mod nim_peer_tests;

pub(super) struct NinePPolicyAdapter {
    startup: FileStartup,
}
impl NinePPolicyAdapter {
    pub(super) fn pending(
        endpoint: sophia_runtime::PolicyRoleEndpoint,
        supervisor: &sophia_runtime::ProcessSupervisor,
        epoch: u64,
        limits: WmFileLimits,
        qids: WmQids,
    ) -> Result<Self, String> {
        Ok(Self {
            startup: FileStartup::pending(endpoint, supervisor, epoch, limits, qids)?,
        })
    }
    pub(super) fn supplied(
        stream: UnixStream,
        epoch: u64,
        limits: WmFileLimits,
        qids: WmQids,
    ) -> Result<Self, String> {
        Ok(Self {
            startup: FileStartup::adopt(stream, epoch, limits, qids)?,
        })
    }
    fn run<T>(
        &mut self,
        operation: impl FnOnce(&mut NinePReactor<TypedFileCodec>) -> Result<T, String>,
    ) -> Result<T, String> {
        let result = self.startup.reactor_mut().and_then(operation);
        if result.is_err() {
            self.startup.close();
        }
        result
    }
}
impl PolicyAdapter for NinePPolicyAdapter {
    fn admit(
        &mut self,
        admission: PolicyAdmissionPermit,
        epoch: u64,
        profile: Option<PolicyProfileAdmission>,
    ) -> Result<(), String> {
        if epoch != self.startup.epoch() {
            self.startup.close();
            return Err("WM file admitted epoch mismatch".into());
        }
        self.startup.admit(admission, profile)
    }
    fn selected_capabilities(&self) -> u64 {
        self.startup
            .reactor()
            .ok()
            .and_then(|r| r.server.export().selected_capabilities())
            .unwrap_or(0)
    }
    fn receive_within(
        &mut self,
        permit: PolicyReceivePermit,
        timeout: Duration,
    ) -> Result<PolicyAdapterEvent, String> {
        self.run(|reactor| {
            reactor
                .receive(permit, timeout)?
                .ok_or_else(|| "WM file response deadline expired".into())
        })
    }
    fn try_receive(
        &mut self,
        permit: PolicyReceivePermit,
    ) -> Result<Option<PolicyAdapterEvent>, String> {
        self.run(|reactor| reactor.receive(permit, Duration::ZERO))
    }
    fn send(&mut self, command: &PolicyTransportCommand) -> Result<(), String> {
        let deadline = Instant::now() + SEND_DEADLINE;
        let epoch = self.startup.epoch();
        if matches!(command, PolicyTransportCommand::Stop) {
            self.disconnect();
            return Ok(());
        }
        self.run(|reactor| {
            let selected = reactor
                .server
                .export()
                .selected_capabilities()
                .ok_or("WM file not negotiated")?;
            match command {
                PolicyTransportCommand::ConfigurationOutcome {
                    transaction,
                    generation,
                    outcome,
                } => {
                    let value = WmFileConfigurationOutcome {
                        transaction: *transaction,
                        generation: *generation,
                        outcome: *outcome,
                    };
                    reactor.send_encoded_before(
                        WmFileKind::ConfigurationOutcome,
                        deadline,
                        |header| {
                            encode_wm_file_configuration_outcome(header, &value, selected)
                                .map_err(codec_error)
                        },
                    )
                }
                PolicyTransportCommand::Cycle {
                    snapshot_transaction,
                    request_transaction,
                    scene,
                    actions,
                    classifications,
                    launch_origins,
                    request,
                } => {
                    check_publication(&reactor.stopped, deadline)
                        .map_err(|e| format!("WM file cycle: {e:?}"))?;
                    // This transient typed copy precedes journal credit. The
                    // bounded encoded snapshot is allocated only after credit;
                    // neither representation enters another command queue.
                    let snapshot = WmFileSnapshot {
                        transaction: *snapshot_transaction,
                        snapshot: PolicyDecodedSnapshot {
                            scene: scene.as_ref().clone(),
                            actions: actions.clone(),
                            classifications: classifications.clone(),
                            launch_origins: launch_origins.clone(),
                        },
                    };
                    let cycle = WmFileCycle {
                        snapshot_transaction: *snapshot_transaction,
                        request_transaction: *request_transaction,
                        request: request.clone(),
                    };
                    reactor.send_cycle(&snapshot, &cycle, deadline)
                }
                PolicyTransportCommand::ProjectionOutcome {
                    transaction,
                    request_id,
                    scene_generation,
                    outcome,
                    expect_session_operation,
                } => {
                    let value = WmFileProjectionOutcome {
                        transaction: *transaction,
                        request_id: *request_id,
                        scene_generation: *scene_generation,
                        outcome: *outcome,
                        expect_session_operation: *expect_session_operation,
                    };
                    reactor.send_encoded_before(WmFileKind::ProjectionOutcome, deadline, |header| {
                        encode_wm_file_projection_outcome(header, &value, selected)
                            .map_err(codec_error)
                    })
                }
                PolicyTransportCommand::SessionOperationOutcome {
                    transaction,
                    request_id,
                    outcome,
                } => {
                    let value = WmFileSessionOperationOutcome {
                        transaction: *transaction,
                        outcome: PolicySessionOperationOutcome {
                            connection_epoch: epoch,
                            request_id: *request_id,
                            outcome: *outcome,
                        },
                    };
                    reactor.send_encoded_before(
                        WmFileKind::SessionOperationOutcome,
                        deadline,
                        |header| {
                            encode_wm_file_session_operation_outcome(header, &value, selected)
                                .map_err(codec_error)
                        },
                    )
                }
                PolicyTransportCommand::PresentationReceipt {
                    transaction,
                    receipt,
                } => {
                    let value = WmFilePresentationReceipt {
                        transaction: *transaction,
                        receipt: *receipt,
                    };
                    reactor.send_encoded_before(
                        WmFileKind::PresentationReceipt,
                        deadline,
                        |header| {
                            encode_wm_file_presentation_receipt(header, &value, selected)
                                .map_err(codec_error)
                        },
                    )
                }
                PolicyTransportCommand::Stop => Ok(()), // Already closed above; no file event.
            }
        })
    }
    fn stop_handle(&self) -> Option<Box<dyn PolicyAdapterStop>> {
        Some(self.startup.stop_handle())
    }
    fn disconnect(&mut self) {
        self.startup.close();
    }
}
