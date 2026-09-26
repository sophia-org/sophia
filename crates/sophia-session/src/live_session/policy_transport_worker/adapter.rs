//! Transport boundary for the existing WM driver. These are semantic records;
//! adapters own framing/assembly, not proposal settlement or scene state.
use super::driver::PolicyReceivePermit;
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
    fn disconnect(&mut self);
}

pub(super) trait PolicyAdapterStop: Send + Sync {
    fn stop(&self);
}
