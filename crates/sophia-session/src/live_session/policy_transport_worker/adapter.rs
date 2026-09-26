//! Transport boundary for the existing WM driver. These are semantic records;
//! adapters own framing/assembly, not proposal settlement or scene state.
use super::driver::{PolicyAdmissionPermit, PolicyReceivePermit};
use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct PolicyProfileAdmission {
    pub connection_epoch: u64,
    pub generation: u64,
    pub digest: [u8; 32],
    pub prepare_transaction: TransactionId,
    pub activate_transaction: TransactionId,
}

pub(super) enum PolicyAdapterEvent {
    Negotiation(sophia_protocol::wm_files::WmFileNegotiationOffer),
    ProfileCompletion {
        kind: sophia_runtime::PolicyProfileHandoffKind,
        completion: sophia_protocol::PolicyProfileCompletion,
    },
    ProjectionPending,
    Projection(Box<PolicyProjectionProposal>),
    /// Completed bytes failed semantic decoding. The driver still owns phase
    /// refusal precedence, just as it did before decoding moved to the adapter.
    MalformedProjection(String),
    ProjectionDiscarded,
    Configuration {
        transaction: TransactionId,
        configuration: PolicyConfiguration,
    },
    Dirty(sophia_protocol::PolicyDirtyRequest),
    SessionOperation {
        transaction: TransactionId,
        request: PolicySessionOperationRequest,
    },
    UnexpectedProfileCompletion,
}

pub(super) trait PolicyAdapter: Send + 'static {
    /// Success includes profile prepare/activate when requested. The driver
    /// cannot publish Negotiated before this admission finishes.
    fn admit(
        &mut self,
        admission: PolicyAdmissionPermit,
        connection_epoch: u64,
        profile: Option<PolicyProfileAdmission>,
    ) -> Result<(), String>;
    fn selected_capabilities(&self) -> u64;
    fn receive_within(
        &mut self,
        permit: PolicyReceivePermit,
        timeout: Duration,
    ) -> Result<PolicyAdapterEvent, String>;
    fn try_receive(
        &mut self,
        permit: PolicyReceivePermit,
    ) -> Result<Option<PolicyAdapterEvent>, String>;
    fn send(&mut self, command: &PolicyTransportCommand) -> Result<(), String>;
    /// Optional transport wakeup, never a second phase/settlement owner.
    /// Existing IPC retains its socket-bound shutdown behavior.
    fn stop_handle(&self) -> Option<Box<dyn PolicyAdapterStop>> {
        None
    }
    /// Opts into socket/readiness-driven idle service. The bell only wakes
    /// the driver; accepted commands remain in its existing bounded queue.
    /// None preserves current IPC's recv_timeout/try_receive path.
    /// Returning Some requires overriding idle_receive with a blocking single
    /// turn. An incomplete opt-in fails closed instead of busy-polling.
    fn command_wake_handle(&self) -> Option<Box<dyn PolicyAdapterCommandWake>> {
        None
    }
    /// Exactly one blocking transport turn, then return to the command queue
    /// even when no semantic event was delivered. Never an active response wait.
    fn idle_receive(
        &mut self,
        _permit: PolicyReceivePermit,
        _cap: Duration,
    ) -> Result<Option<PolicyAdapterEvent>, String> {
        Err("adapter does not support readiness-driven idle".into())
    }
    fn disconnect(&mut self);
}

pub(super) trait PolicyAdapterStop: Send + Sync {
    fn stop(&self);
}

pub(super) trait PolicyAdapterCommandWake: Send + Sync {
    fn wake(&self);
}
