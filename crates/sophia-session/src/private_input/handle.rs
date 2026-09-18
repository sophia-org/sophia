//! Standing the service up, and what Session keeps afterwards.

use sophia_protocol::ClientAdmissionContext;
use sophia_x_authority::{
    PrivateAdmittedConnection, PrivateDeferredCleanupOutcome, PrivateServiceFailure,
    PrivateUnresolvedEgress, PrivateWorkerCollection, XAuthorityClientControlAck,
    XAuthorityClientInputDelivery, XAuthorityObservedTransactionBatch,
};
use std::path::Path;
use std::time::Duration;

use super::admission::PrivateInputIssueRefusal;
use super::control::{
    PrivateInputCommitted, PrivateInputControl, PrivateInputControlAccepted,
    PrivateInputControlError,
};
use super::submission::{PrivateInputConnection, PrivateInputSubmission};

/// A boundary or a record that could not be read.
///
/// ITS OWN ANSWER, NEVER AN EMPTY ONE. A participant whose lock is poisoned
/// and a boundary holding nothing are opposite facts, and a reader given an
/// empty list for both would conclude the second from the first.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PrivateInputUnavailable;

impl core::fmt::Display for PrivateInputUnavailable {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str("the private input boundary could not be read")
    }
}

impl std::error::Error for PrivateInputUnavailable {}

/// A bounded wait that ran out.
///
/// CARRIES WHAT IT LAST SAW. An expired wait is not a state of the service; it
/// is a fact about the waiting. The readiness observed at expiry travels with
/// it so a caller can tell a service still binding from one that had already
/// stopped, without a second call that would race the answer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrivateInputWaitExpired {
    pub observed: PrivateInputReadiness,
}

impl core::fmt::Display for PrivateInputWaitExpired {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            formatter,
            "the readiness wait expired while the service was {:?}",
            self.observed
        )
    }
}

impl std::error::Error for PrivateInputWaitExpired {}

/// What became of the thread that served.
///
/// SEPARATE FROM WORKER COLLECTION. Per-connection workers are collected by
/// the service; this is the service's own thread. A run that collected every
/// worker and then lost its service thread has not finished, and one number
/// covering both would hide exactly that.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum PrivateInputThreadJoin {
    Joined,
    Panicked(String),
    /// The default, because an outcome built before a thread ran has not
    /// joined one. Reading absence as success is the mistake this avoids.
    #[default]
    NeverStarted,
}

/// What the durable owner still holds.
///
/// A COUNT ALONE PROVES NOTHING. An unreadable store reports no credits, which
/// is indistinguishable from an empty one unless readability is its own fact.
/// Owed and indeterminate work are kept apart from reserved credits for the
/// same reason: settled, owed and unproved are three answers.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PrivateInputSettlement {
    pub readable: bool,
    pub reserved_credits: Option<usize>,
    pub owed: Option<usize>,
    pub indeterminate: Option<usize>,
}

/// Why the service could not be stood up.
#[derive(Debug)]
pub enum PrivateInputRefusal {
    /// The authority instance could not be built with the requested capacity.
    Capacity(sophia_input_authority::CapacityError),
    /// The frontend configuration was refused before anything was bound.
    Configuration(sophia_x_authority::X11SetupSocketError),
    /// The frontend refused construction and handed its parts back.
    Construction(sophia_x_authority::AdmissionRefusal),
    /// The namespace registry would not admit this service's namespace.
    Namespace(sophia_runtime::NamespaceRegistryError),
    /// The service thread could not be started.
    Thread(std::io::Error),
}

/// How far the service has got, as a value rather than a guess.
///
/// BOUNDED AND TYPED. A caller waits for readiness against an absolute
/// deadline and is told which of these it reached, so a service that refused
/// and a service that is merely slow are never the same answer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PrivateInputReadiness {
    /// The thread is up and the listener is not yet accepting.
    Binding,
    /// The listener is accepting on the configured path.
    Ready,
    /// The service refused before it began serving, with its reason.
    Refused(String),
    /// The service has returned or unwound.
    Stopped,
}

/// What the service reported when it ended.
///
/// EVERY OUTCOME IS KEPT APART. A service that failed, work that was left
/// unresolved on the wire, workers that were collected, maintenance that ran
/// afterwards and settlement that is still retained are five different facts.
/// Folding any of them into a single success flag would let a run that lost
/// work look like a run that finished it.
#[derive(Debug, Default)]
pub struct PrivateInputOutcome {
    /// The service's own failure, when it had one. `None` is a service that
    /// returned, not a service that succeeded at everything it owed.
    pub failure: Option<PrivateServiceFailure>,
    pub unresolved_egress: Vec<PrivateUnresolvedEgress>,
    pub workers: Vec<PrivateWorkerCollection>,
    pub maintenance: Vec<PrivateDeferredCleanupOutcome>,
    /// Whether the invocation was interrupted. Reported as itself and never
    /// cleared: an interruption that is later tidied up was still an
    /// interruption, and the budget it closed stays closed.
    pub interrupted: bool,
    /// What became of the serving thread itself, apart from its workers.
    pub service_thread: PrivateInputThreadJoin,
    /// What the durable owner still holds after collection. Not a bare count:
    /// collecting every actor says nothing about whether anything is owed, and
    /// a store that could not be read says less still.
    pub settlement: PrivateInputSettlement,
}

/// What the service is holding right now.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PrivateInputStatus {
    pub readiness_is_ready: bool,
    pub admitted: usize,
    pub grants_issued: usize,
    /// What the store is holding right now. These are counts for diagnosis and
    /// establish no settlement on their own; `stop` reports what was actually
    /// collected and retained.
    pub settlement: PrivateInputSettlement,
    pub interrupted: bool,
}

/// The private input service, as Session stands it up.
pub struct PrivateInputService;

impl PrivateInputService {
    /// Stand the service up and return the handle Session keeps.
    ///
    /// The issuer, the authority instance, the durable store and the service
    /// owner are established here and never leave. What comes back can name
    /// connections, issue submissions, drain receipts and stop; it cannot
    /// reach any of those.
    pub fn start(
        _config: super::PrivateInputConfig,
    ) -> Result<PrivateInputHandle, PrivateInputRefusal> {
        unimplemented!("service thread lands with the keeper work")
    }
}

/// What Session keeps for one running private input service.
pub struct PrivateInputHandle {
    _private: (),
}

impl PrivateInputHandle {
    /// The socket this service is actually listening on.
    ///
    /// REPORTED, NOT RECONSTRUCTED. A client that built this path from the
    /// same parts would agree with the service only for as long as both
    /// recipes stayed identical, and would disagree silently the moment one
    /// changed. It is the configured path, echoed from the service that bound
    /// it.
    pub fn socket_path(&self) -> &Path {
        unimplemented!("service thread lands with the keeper work")
    }

    /// Wait until the service is ready, or until this deadline passes.
    ///
    /// BOUNDED BY AN ABSOLUTE DEADLINE, computed once from this duration, so
    /// a slow readiness cannot be extended indefinitely by repeated partial
    /// progress. Returns what it reached rather than a bare success, so a
    /// caller can tell readiness from a refusal that arrived first.
    pub fn await_ready(
        &self,
        _within: Duration,
    ) -> Result<PrivateInputReadiness, PrivateInputWaitExpired> {
        unimplemented!("service thread lands with the keeper work")
    }

    /// What the service has got to right now, without waiting.
    pub fn readiness(&self) -> PrivateInputReadiness {
        unimplemented!("service thread lands with the keeper work")
    }

    pub fn status(&self) -> PrivateInputStatus {
        unimplemented!("service thread lands with the keeper work")
    }

    /// The connections this boundary currently has admitted.
    ///
    /// FACTS FROM THE BOUNDARY, not Session's idea of them. Each row carries
    /// the exact admission id and connection generation, so a caller names one
    /// connection rather than a client number a successor may have taken.
    pub fn admitted(&self) -> Result<Vec<PrivateAdmittedConnection>, PrivateInputUnavailable> {
        unimplemented!("service thread lands with the keeper work")
    }

    /// Session's own record for one admission, including whether its setup
    /// carried evidence bound to this instance.
    pub fn admission_record(
        &self,
        _admission: sophia_protocol::ClientAdmissionId,
    ) -> Result<Option<super::PrivateInputAdmissionRecord>, PrivateInputUnavailable> {
        unimplemented!("service thread lands with the keeper work")
    }

    /// Submit one Engine-committed control for a named connection.
    ///
    /// SESSION MINTS THE TRANSACTION. The returned value carries it, so the
    /// real acknowledgement that later arrives on the drain is matched against
    /// this exact control rather than against a number the caller guessed.
    /// Nothing here reaches the control producer, the broker or the registry.
    pub fn submit_control(
        &self,
        _connection: PrivateInputConnection,
        _control: PrivateInputControl,
    ) -> Result<PrivateInputControlAccepted, PrivateInputControlError> {
        unimplemented!("service thread lands with the keeper work")
    }

    /// Take the transactions the frontend has observed, commit them through
    /// the headless coordinator, and submit the controls that commit calls for.
    ///
    /// ONE STEP, REPORTED AS THREE NUMBERS. Observed, committed and applied
    /// only agree when nothing was refused, and a step that committed state it
    /// could not then apply is exactly what this reports rather than hides.
    /// The coordinator belongs to the Session owner, not to a caller: there is
    /// no way from here to seed applied state without a real transaction.
    pub fn apply_committed(
        &self,
        _within: Duration,
    ) -> Result<PrivateInputCommitted, PrivateInputUnavailable> {
        unimplemented!("service thread lands with the keeper work")
    }

    /// Issue a submission handle for one exact admitted connection.
    ///
    /// EVERY CONDITION IS CURRENT. Grants must be enabled, the admission must
    /// have presented evidence for this instance, the registry must still hold
    /// it as the current admission, and the boundary must still have a live
    /// connection for it. The expected admission then travels into the act
    /// that issues the grant, so a number reused between this call and that
    /// act is refused there rather than served.
    ///
    /// The ingress is issued once and retained inside the returned handle, so
    /// the grant, the device and the completion cell continue across every
    /// request the adapter makes.
    pub fn issue(
        &self,
        _context: ClientAdmissionContext,
        _device: sophia_protocol::DeviceId,
    ) -> Result<PrivateInputSubmission, PrivateInputIssueRefusal> {
        unimplemented!("service thread lands with the keeper work")
    }

    /// Revoke one admission and retire exactly the grants it authorised.
    pub fn revoke(
        &self,
        _context: ClientAdmissionContext,
    ) -> Result<PrivateInputConnection, PrivateInputIssueRefusal> {
        unimplemented!("service thread lands with the keeper work")
    }

    /// Take the delivery receipts that have arrived, without waiting.
    ///
    /// DRAINED, NEVER PROBED AWAY. Each call returns what is queued and
    /// leaves nothing behind; a caller that wants to wait supplies a bound to
    /// `drain_deliveries_within`. Asking whether anything is there is the same
    /// act as taking it, so there is no separate question that consumes.
    pub fn drain_deliveries(&self) -> Vec<XAuthorityClientInputDelivery> {
        unimplemented!("service thread lands with the keeper work")
    }

    /// Take delivery receipts, waiting up to this bound for the first one.
    pub fn drain_deliveries_within(&self, _within: Duration) -> Vec<XAuthorityClientInputDelivery> {
        unimplemented!("service thread lands with the keeper work")
    }

    pub fn drain_acknowledgements(&self) -> Vec<XAuthorityClientControlAck> {
        unimplemented!("service thread lands with the keeper work")
    }

    pub fn drain_acknowledgements_within(
        &self,
        _within: Duration,
    ) -> Vec<XAuthorityClientControlAck> {
        unimplemented!("service thread lands with the keeper work")
    }

    pub fn drain_transactions(&self) -> Vec<XAuthorityObservedTransactionBatch> {
        unimplemented!("service thread lands with the keeper work")
    }

    pub fn drain_transactions_within(
        &self,
        _within: Duration,
    ) -> Vec<XAuthorityObservedTransactionBatch> {
        unimplemented!("service thread lands with the keeper work")
    }

    /// Stop the service and collect it.
    ///
    /// Producer admission closes first, then the invocation is interrupted and
    /// its actors collected, and only then does the maintenance this keeper is
    /// still allowed to perform run. The keeper stays on the thread it was
    /// made on throughout.
    pub fn stop(self) -> PrivateInputOutcome {
        unimplemented!("service thread lands with the keeper work")
    }
}

impl Drop for PrivateInputHandle {
    /// A handle that goes without `stop` still stops the service.
    ///
    /// GRACEFUL, AND THE SAME ORDER. Dropping is not a way to skip closing
    /// producer admission or collecting actors; it performs the same stop and
    /// discards only the report. A channel that has already been lost does not
    /// change that: the owner and the keeper outlive the invocation, so the
    /// stop still reaches them.
    fn drop(&mut self) {}
}
