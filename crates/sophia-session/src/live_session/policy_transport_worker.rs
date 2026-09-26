use std::sync::mpsc::{
    Receiver, RecvTimeoutError, SyncSender, TryRecvError, TrySendError, sync_channel,
};
use std::thread::JoinHandle;
use std::time::Duration;

use sophia_protocol::{
    PolicyActionRegistration, PolicyConfiguration, PolicyProjectionOutcome,
    PolicyProjectionProposal, PolicyProjectionRequest, PolicySceneSnapshot,
    PolicySessionOperationRequest, TransactionId,
};
mod adapter;
mod current_ipc;
mod driver;
// File transport remains explicitly selected by the Session owner.
#[cfg_attr(not(test), allow(dead_code))]
pub(super) mod ninep;
use adapter::{PolicyAdapter, PolicyAdapterStop, PolicyProfileAdmission};
use driver::run_policy_transport;

/// Opaque logical filesystem identity custody; retained by Session across
/// worker replacement, never reconstructed from an endpoint path or epoch.
#[derive(Clone)]
pub(super) struct PolicyFilesystemQids(ninep::WmQids);
impl PolicyFilesystemQids {
    pub(super) fn new() -> Self {
        Self(ninep::WmQids::new())
    }
}

const POLICY_TRANSPORT_CAPACITY: usize = 1;

pub(super) enum PolicyTransportCommand {
    ConfigurationOutcome {
        transaction: TransactionId,
        generation: u64,
        outcome: PolicyProjectionOutcome,
    },
    Cycle {
        snapshot_transaction: TransactionId,
        request_transaction: TransactionId,
        /// Boxed because the scene is most of this variant, and every other
        /// command in the same queue would otherwise be sized by it.
        scene: Box<PolicySceneSnapshot>,
        actions: Vec<PolicyActionRegistration>,
        classifications: Vec<sophia_protocol::PolicySurfaceClassification>,
        launch_origins: Vec<sophia_protocol::PolicyLaunchContext>,
        request: PolicyProjectionRequest,
    },
    ProjectionOutcome {
        transaction: TransactionId,
        request_id: u64,
        scene_generation: u64,
        outcome: PolicyProjectionOutcome,
        expect_session_operation: bool,
    },
    SessionOperationOutcome {
        transaction: TransactionId,
        request_id: u64,
        outcome: PolicyProjectionOutcome,
    },
    PresentationReceipt {
        transaction: TransactionId,
        receipt: sophia_protocol::PolicyPresentationReceipt,
    },
    Stop,
}

pub(super) enum PolicyTransportEvent {
    Negotiated,
    ReadyForCycle {
        capabilities: u64,
    },
    Configuration {
        transaction: TransactionId,
        configuration: PolicyConfiguration,
    },
    Dirty(sophia_protocol::PolicyDirtyRequest),
    Projection(Box<PolicyProjectionProposal>),
    SessionOperation {
        transaction: TransactionId,
        request: PolicySessionOperationRequest,
    },
    Failed(String),
}

pub(super) struct PolicyTransportWorker {
    commands: Option<SyncSender<PolicyTransportCommand>>,
    events: Receiver<PolicyTransportEvent>,
    thread: Option<JoinHandle<()>>,
    stop: Option<Box<dyn PolicyAdapterStop>>,
}

impl PolicyTransportWorker {
    fn spawn(
        mut transport: impl PolicyAdapter,
        connection_epoch: u64,
        profile_admission: Option<PolicyProfileAdmission>,
    ) -> Result<Self, std::io::Error> {
        let stop = transport.stop_handle();
        let (command_sender, command_receiver) = sync_channel(POLICY_TRANSPORT_CAPACITY);
        let (event_sender, event_receiver) = sync_channel(POLICY_TRANSPORT_CAPACITY);
        let thread = std::thread::Builder::new()
            .name("sophia-policy-v1".to_owned())
            .spawn(move || {
                let result = run_policy_transport(
                    &mut transport,
                    connection_epoch,
                    profile_admission,
                    &command_receiver,
                    &event_sender,
                );
                if let Err(error) = result {
                    let _ = event_sender.try_send(PolicyTransportEvent::Failed(error));
                }
                transport.disconnect();
            })?;
        Ok(Self {
            commands: Some(command_sender),
            events: event_receiver,
            thread: Some(thread),
            stop,
        })
    }

    pub(super) fn try_command(
        &self,
        command: PolicyTransportCommand,
    ) -> Result<(), PolicyTransportCommand> {
        if matches!(command, PolicyTransportCommand::Stop)
            && let Some(stop) = &self.stop
        {
            stop.stop();
            return Ok(());
        }
        let Some(commands) = self.commands.as_ref() else {
            return Err(command);
        };
        match commands.try_send(command) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(command) | TrySendError::Disconnected(command)) => Err(command),
        }
    }

    pub(super) fn try_event(&self) -> Result<Option<PolicyTransportEvent>, ()> {
        match self.events.try_recv() {
            Ok(event) => Ok(Some(event)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => Err(()),
        }
    }

    pub(super) fn event_timeout(
        &self,
        timeout: Duration,
    ) -> Result<PolicyTransportEvent, RecvTimeoutError> {
        self.events.recv_timeout(timeout)
    }
}

impl Drop for PolicyTransportWorker {
    fn drop(&mut self) {
        // An adapter can be waiting for transport retention credit while the
        // command queue is full. Wake that wait independently of queue space.
        if let Some(stop) = &self.stop {
            stop.stop();
        }
        // A producer may be blocked on the one-slot event queue when the owner
        // retires this worker. Disconnect that queue before joining it.
        let (_, closed_events) = sync_channel(POLICY_TRANSPORT_CAPACITY);
        drop(std::mem::replace(&mut self.events, closed_events));
        if let Some(commands) = self.commands.take() {
            let _ = commands.try_send(PolicyTransportCommand::Stop);
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// How long a policy client may take to answer before it is treated as gone.
///
/// The socket's own timeout is one window; this is the budget across several.
/// A client legitimately needs more than one window after a topology change,
/// which hands it an entire new layout to compute, and one expired window used
/// to restart a window manager that was merely busy.
const POLICY_CLIENT_RESPONSE_DEADLINE: Duration = Duration::from_secs(12);

#[path = "../../tests/support/control_worker_shutdown.rs"]
mod control_worker_shutdown;

#[path = "../../tests/support/policy_adapter_driver.rs"]
mod adapter_driver_tests;

#[path = "../../tests/support/policy_adapter_stop.rs"]
mod adapter_stop_tests;
