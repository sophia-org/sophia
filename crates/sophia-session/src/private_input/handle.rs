//! Standing the service up, and what Session keeps afterwards.

use sophia_protocol::ClientAdmissionContext;
use sophia_x_authority::{
    PrivateAdmittedConnection, PrivateDeferredCleanupOutcome, PrivateServiceFailure,
    PrivateUnresolvedEgress, PrivateWorkerCollection, XAuthorityClientControlAck,
    XAuthorityClientInputDelivery, XAuthorityObservedTransactionBatch,
};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use super::admission::PrivateInputIssueRefusal;
use super::control::{PrivateInputAction, PrivateInputControlError, PrivateInputSubmitted};
use super::submission::{PrivateInputConnection, PrivateInputSubmission};

/// The most one drain takes at once.
///
/// BOUNDED, AND THE TAIL STAYS QUEUED. A live producer can fill a channel as
/// fast as a reader empties it, so draining until empty is a loop with no
/// promise of ending. Whatever is left stays in its own channel for the next
/// call rather than being dropped.
pub const PRIVATE_INPUT_DRAIN_BOUND: usize = 256;

/// The most receipts this service will hold in its own custody.
///
/// SEPARATE FROM THE PER-CALL BOUND. One bounds how much work a single call
/// does; this bounds how much a service can be holding at once. Reaching it
/// leaves the remainder in the channel, which is a queue already.
pub const PRIVATE_INPUT_RECEIPT_RETENTION: usize = 256;

/// Receipts taken from the delivery channel, and what became of each.
///
/// TWO DIFFERENT OUTCOMES, KEPT APART. An observed receipt released its
/// delivery's place in the ledger. A retained one did not, is still held, and
/// will be offered again; it is owed work rather than a receipt that was dealt
/// with, and one list holding both would make those the same answer.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PrivateInputReceipts {
    /// Handed back to the ledger, which pruned the ticket.
    pub observed: Vec<XAuthorityClientInputDelivery>,
    /// How many receipts this service is still holding, across every call.
    ///
    /// INVENTORY, NOT THIS CALL'S WORK, AND A COUNT RATHER THAN A LIST. The
    /// observations above are bounded; this is not, so returning the receipts
    /// themselves would make an unbounded copy of everything held on every
    /// call, and would blur the one distinction that matters here -- what this
    /// call did against what the service owes.
    pub retained: usize,
    /// How many receipts this call actually looked at.
    ///
    /// THE BOUND IS ON THIS, AND IT HAS TO BE VISIBLE. An earlier version
    /// bounded the work by what was left at the end of the call, which let a
    /// full retention queue be observed and a full channel drained on top of
    /// it in one call -- twice the advertised bound, and invisible from the
    /// outside. Reporting what was visited is what makes the bound checkable.
    pub visited: usize,
    /// How many observations could not be made because the ledger could not be
    /// read.
    ///
    /// COUNTED APART FROM THE REST OF `retained`. A ledger that was never asked
    /// has refused nothing, and a reader that could not tell this from a
    /// declined receipt would conclude the ledger rejected work it never saw.
    pub unreadable: usize,
}

/// How many receipts this service is holding, and whether that is all of them.
///
/// A PARTIAL COUNT THAT SAID SO IS USABLE; ONE THAT DID NOT IS WORSE THAN
/// NOTHING. The retention bound can stop the channel being emptied, and a
/// caller told only a number would read the number as the whole obligation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PrivateInputReceiptInventory {
    /// Receipts in this service's own custody.
    pub retained: usize,
    /// Whether the delivery channel was emptied into that custody. When false,
    /// `retained` is a floor and not a total.
    pub complete: bool,
}

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

/// What one maintenance visit reported.
///
/// A VISIT, NOT A TALLY. A stop drives a bounded number of attempts, and
/// reaching that bound means the loop stopped asking rather than that anything
/// settled. Each visit's own phase, status and allowance refusal are kept so a
/// reader can see which of those it was.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PrivateInputVisit {
    pub phase: sophia_x_authority::PrivateMaintenancePhase,
    pub status: sophia_x_authority::PrivateMaintenanceStatus,
    pub allowance_refusal: Option<sophia_input_authority::ServiceStartRefusal>,
}

/// How the invocation itself ended.
///
/// DISTINCT FROM THE THREAD'S FATE. An invocation can unwind while its thread
/// goes on to run maintenance and join cleanly, and a thread can fail outside
/// any invocation. Reporting one as the other makes either story unreadable.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum PrivateInputInvocation {
    #[default]
    Returned,
    Failed,
    /// It panicked, with the payload kept rather than reduced to a flag.
    Unwound(String),
}

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
    /// The serving thread reported its own construction refusal before the
    /// service began. Kept apart from `Construction`, which is this side's
    /// reading when the thread said nothing at all.
    ConstructionRefused(String),
    /// The namespace registry would not admit this service's namespace.
    Namespace(sophia_runtime::NamespaceRegistryError),
    /// The configured topology cannot be served as stated. Refused rather than
    /// substituted, because an invented output would commit geometry against a
    /// screen nobody asked for.
    Topology(PrivateInputTopologyRefusal),
    /// The durable store's place count cannot be formed from the configured
    /// client bound.
    ///
    /// REFUSED BEFORE ANYTHING IS BUILT, not discovered by an arithmetic wrap.
    /// A bound whose place count overflows would, wrapped, produce a store
    /// sized smaller than the connections it is meant to hold -- which is the
    /// one failure a capacity is there to prevent.
    SettlementCapacity { clients: usize, places_each: usize },
    /// The lifetime offered is already running a service.
    ///
    /// ONE SLOT, ONE SERVICE. A lifetime reserves exactly one home for one
    /// service's unresolved runtime; admitting a second would give two
    /// services one home between them.
    LifetimeInUse,
    /// The lifetime offered is still holding a service that ended with work
    /// owed. Starting another under it would leave that work with nowhere to
    /// be, and there is deliberately no call that empties it.
    LifetimeRetainsUnresolved,
    /// The lifetime's slot could not be read, so nothing can be admitted under
    /// it: a service whose custody has nowhere to go must not be started.
    LifetimeUnreadable,
    /// The service thread could not be started.
    Thread(std::io::Error),
}

/// Why a configured topology cannot be served.
///
/// NAMED, NOT COLLAPSED. "The topology was refused" leaves an operator
/// guessing; each of these is a different thing to go and fix.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrivateInputTopologyRefusal {
    /// The topology declares no outputs at all, so there is no coordinate
    /// space to route into.
    NoOutputs,
    /// The topology names a primary that is not among its own outputs.
    ///
    /// REFUSED, NOT REPAIRED. An earlier version fell back to the first output
    /// here, which silently served a different screen from the one configured
    /// and made every committed geometry a claim about the wrong output.
    PrimaryAbsent { primary: sophia_protocol::OutputId },
    /// The topology declares more than one output.
    ///
    /// REFUSED BECAUSE THIS ASSEMBLY CANNOT HONOUR IT. The headless assembly
    /// ticks exactly one output: its per-output frame clock is advanced only
    /// for the headless engine's own output. Admitting a head for every
    /// declared output would produce heads that are never ticked and never
    /// present, which is a narrowing of the configuration dressed up as
    /// support for it. A multihead private service needs a backend that drives
    /// more than one output, and until there is one this says so plainly.
    MultipleOutputs { count: usize },
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
#[derive(Default)]
pub struct PrivateInputOutcome {
    /// How the invocation ended, as itself.
    pub invocation: PrivateInputInvocation,
    /// The service's own failure, when it had one. `None` is a service that
    /// returned, not a service that succeeded at everything it owed.
    pub failure: Option<PrivateServiceFailure>,
    pub unresolved_egress: Vec<PrivateUnresolvedEgress>,
    /// The workers this stop actually joined. `None` when the invocation made
    /// no report, which an unwind does not: an empty vector would claim it
    /// collected none, and that is a different statement.
    pub workers: Option<Vec<PrivateWorkerCollection>>,
    /// What the execution is once the serving thread has been joined.
    ///
    /// READ AFTER THE JOIN, FROM A WITNESS THAT OUTLIVES THE THREAD. An earlier
    /// version reported the reading the thread itself took before its keeper
    /// dropped, which said the execution was retained however the thread then
    /// ended -- so a joined thread reported a live execution. A joined thread
    /// does not imply a live execution, and this no longer says it does.
    /// Absent when preparation never reached an execution owner; never
    /// fabricated from a prior reading.
    pub execution: Option<sophia_x_authority::PrivateExecutionReading>,
    /// What the keeper held when the invocation ended, before maintenance and
    /// before the keeper's own drop.
    ///
    /// KEPT AS ITS OWN FACT rather than folded into `execution`. The two
    /// answer different questions -- what the invocation left behind, and what
    /// survived the thread -- and a run where they disagree is exactly the
    /// story worth reading.
    pub execution_at_close: Option<sophia_x_authority::PrivateExecutionReading>,
    /// The durable owner, kept alive by this outcome while anything is still
    /// owed. Private because it is custody, not a report: a reader can ask
    /// whether obligations remain, and cannot take them. Skipped in `Debug`
    /// because printing custody is not reporting it.
    pub(super) retained: Option<Arc<super::service::PrivateInputRuntime>>,
    pub maintenance: Vec<PrivateDeferredCleanupOutcome>,
    /// What each maintenance visit reported, in order.
    pub visits: Vec<PrivateInputVisit>,
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
    /// Committed decisions this service never managed to hand to the order.
    ///
    /// ITS OWN FACT, BECAUSE THE STORE HAS NEVER SEEN IT. This work was
    /// committed by the coordinator and refused or never reached by the order,
    /// so it is owed and yet appears nowhere in the settlement the X store
    /// reports. An outcome that showed only the store would call this nothing.
    /// Work taken from the channel and never handed to the X store, across
    /// every phase the bridge owns.
    ///
    /// `None` MEANS UNREADABLE, NOT NONE OWED. A poisoned bridge reported as
    /// zero would read exactly like a bridge that owed nothing, which is the
    /// one reading that must never be produced by a failure to look.
    pub bridge_undelivered: Option<usize>,
    /// Receipts this service took from the delivery channel and never handed
    /// back to the ledger that issued them.
    ///
    /// EACH ONE IS A DELIVERY PLACE STILL SPENT. `None` means unreadable, for
    /// the same reason it does above: a failure to look is not a finding of
    /// nothing.
    pub receipts_unobserved: Option<usize>,
}

impl core::fmt::Debug for PrivateInputOutcome {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("PrivateInputOutcome")
            .field("invocation", &self.invocation)
            .field("failure", &self.failure)
            .field("unresolved_egress", &self.unresolved_egress)
            .field("workers", &self.workers)
            .field("execution", &self.execution)
            .field("execution_at_close", &self.execution_at_close)
            .field("service_thread", &self.service_thread)
            .field("maintenance", &self.maintenance)
            .field("visits", &self.visits)
            .field("interrupted", &self.interrupted)
            .field("settlement", &self.settlement)
            .field("bridge_undelivered", &self.bridge_undelivered)
            .field("receipts_unobserved", &self.receipts_unobserved)
            .field("retains_obligations", &self.retains_obligations())
            .finish()
    }
}

impl PrivateInputOutcome {
    /// Whether anything is still owed, and therefore still held.
    ///
    /// AN OUTCOME THAT SAYS YES IS KEEPING THE DURABLE OWNER ALIVE. Stopping a
    /// service does not discharge what it owed, and letting the store go with
    /// the facade would destroy the record rather than settle it. An
    /// unreadable store counts as owed here, because a store that cannot be
    /// asked has not said it is empty.
    pub fn retains_obligations(&self) -> bool {
        self.retained.is_some()
    }

    pub(super) fn with_retention(
        mut self,
        runtime: Arc<super::service::PrivateInputRuntime>,
    ) -> Self {
        let settlement = self.settlement;
        let outstanding = !settlement.readable
            || settlement.reserved_credits.is_some_and(|held| held > 0)
            || settlement.owed.is_some_and(|held| held > 0)
            || settlement.indeterminate.is_some_and(|held| held > 0)
            // Unreadable retains, for the same reason an unreadable
            // settlement does: nothing has been shown to be finished.
            || self.bridge_undelivered.is_none_or(|owed| owed > 0)
            || self.receipts_unobserved.is_none_or(|owed| owed > 0);
        if !outstanding {
            // FINISHED CLEAN, SO THE CLAIM GOES BACK. Nothing is retained, so
            // there is nothing to dispose of; the lifetime becomes usable
            // again rather than being spent by a service that owed nothing.
            if let Some(closing) = runtime.closing.upgrade() {
                closing.release_claim();
            }
            return self;
        }
        // THE RESERVED SLOT FIRST. Custody belongs to the lifetime, which was
        // established before the service was, rather than to this report --
        // a caller that drops the report would otherwise drop the custody with
        // it, and a controller that goes without a stop would have nowhere to
        // put anything at all.
        //
        // Placing it settles nothing. It records that this service ended with
        // work outstanding, and it is never read as evidence that any of that
        // work was resolved.
        if let Some(closing) = runtime.closing.upgrade() {
            closing.place(Arc::clone(&runtime));
        }
        self.retained = Some(runtime);
        self
    }
}

/// What the service is holding right now.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PrivateInputStatus {
    pub readiness_is_ready: bool,
    /// What the boundary currently holds. `None` is a boundary that could not
    /// be read, which is not the same as one holding nothing.
    pub admitted: Option<usize>,
    pub grants_issued: Option<usize>,
    /// What the store is holding right now. These are counts for diagnosis and
    /// establish no settlement on their own; `stop` reports what was actually
    /// collected and retained.
    pub settlement: PrivateInputSettlement,
    /// Whether the budget was interrupted, when that is known.
    ///
    /// `None` WHILE THE SERVICE RUNS. An interruption is established by a
    /// maintenance visit refusing, and those happen at stop. Reporting `false`
    /// here would be a claim this has no way to make.
    pub interrupted: Option<bool>,
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
    /// Stand a service up under an explicit lifetime.
    ///
    /// THE LIFETIME IS A PARAMETER RATHER THAN SOMETHING THIS MAKES. Custody
    /// of a service that ends with work owed has to have a home reserved
    /// before the service exists, and a lifetime this constructed would be one
    /// the caller could not hold afterwards.
    pub fn start(
        lifetime: &super::PrivateInputLifetimeOwner,
        config: super::PrivateInputConfig,
    ) -> Result<PrivateInputHandle, PrivateInputRefusal> {
        lifetime.start(config)
    }
}

/// What Session keeps for one running private input service.
pub struct PrivateInputHandle {
    pub(super) runtime: Arc<super::service::PrivateInputRuntime>,
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
        &self.runtime.socket_path
    }

    /// Wait until the service is ready, or until this deadline passes.
    ///
    /// BOUNDED BY AN ABSOLUTE DEADLINE, computed once from this duration, so
    /// a slow readiness cannot be extended indefinitely by repeated partial
    /// progress. Returns what it reached rather than a bare success, so a
    /// caller can tell readiness from a refusal that arrived first.
    pub fn await_ready(
        &self,
        within: Duration,
    ) -> Result<PrivateInputReadiness, PrivateInputWaitExpired> {
        // ONE DEADLINE, COMPUTED ONCE. Recomputing it each turn would let a
        // service that keeps almost arriving extend the wait indefinitely.
        let deadline = std::time::Instant::now() + within;
        loop {
            let observed = self.runtime.readiness();
            if observed != PrivateInputReadiness::Binding {
                return Ok(observed);
            }
            if std::time::Instant::now() >= deadline {
                return Err(PrivateInputWaitExpired { observed });
            }
            std::thread::yield_now();
        }
    }

    /// What the service has got to right now, without waiting.
    pub fn readiness(&self) -> PrivateInputReadiness {
        self.runtime.readiness()
    }

    pub fn status(&self) -> PrivateInputStatus {
        let admitted = self.runtime.participant.admitted().ok();
        PrivateInputStatus {
            readiness_is_ready: self.runtime.readiness() == PrivateInputReadiness::Ready,
            admitted: admitted.as_ref().map(Vec::len),
            grants_issued: admitted
                .as_ref()
                .map(|rows| rows.iter().map(|seen| seen.grants).sum()),
            settlement: self.runtime.settlement(),
            interrupted: None,
        }
    }

    /// The connections this boundary currently has admitted.
    ///
    /// FACTS FROM THE BOUNDARY, not Session's idea of them. Each row carries
    /// the exact admission id and connection generation, so a caller names one
    /// connection rather than a client number a successor may have taken.
    pub fn admitted(&self) -> Result<Vec<PrivateAdmittedConnection>, PrivateInputUnavailable> {
        self.runtime
            .participant
            .admitted()
            .map_err(|_| PrivateInputUnavailable)
    }

    /// Session's own record for one admission, including whether its setup
    /// carried evidence bound to this instance.
    pub fn admission_record(
        &self,
        admission: sophia_protocol::ClientAdmissionId,
    ) -> Result<Option<super::PrivateInputAdmissionRecord>, PrivateInputUnavailable> {
        self.runtime
            .admitted
            .lock()
            .map(|held| held.get(&admission).copied())
            .map_err(|_| PrivateInputUnavailable)
    }

    /// Submit one focus action Session decided on its own authority.
    ///
    /// POLICY, NOT COMMITTED STATE. Focus is Session's to choose, so it is
    /// submitted here and its real acknowledgement checked on the drain. Map
    /// and configure are deliberately absent: those belong to a commit, and
    /// `apply_committed` is the only thing that emits them.
    ///
    /// Session mints the transaction, so the acknowledgement is matched
    /// against this exact submission rather than a number the caller guessed.
    /// Issue a submission handle for one exact admitted connection.
    ///
    /// EVERY CONDITION IS CURRENT. Grants must be enabled, the admission must
    /// have presented evidence for this instance, the registry must still hold
    /// it as current, and the boundary must still have a live connection for
    /// it. The expected admission then travels into the act that issues the
    /// grant, so a number reused between this call and that act is refused
    /// there rather than served. No Session lock is held across the port wait.
    ///
    /// The ingress is issued once and retained, so the grant, the device and
    /// the completion cell continue across every request the adapter makes.
    pub fn issue(
        &self,
        context: ClientAdmissionContext,
        device: sophia_protocol::DeviceId,
    ) -> Result<PrivateInputSubmission, PrivateInputIssueRefusal> {
        let live = self
            .runtime
            .participant
            .admitted()
            .map_err(|_| PrivateInputIssueRefusal::Unavailable)?;
        // THE ROW THE BOUNDARY MATCHED, rather than fields assembled from the
        // admission context. The boundary initialises a binding's generation
        // from that context's auth provenance, so the number would agree; what
        // the row adds is that the boundary is actually holding this admission.
        // The exact admission is what separates a reconnecting client from the
        // one that went.
        let seen = super::admission::may_issue(
            self.runtime.grants,
            &self.runtime.admitted,
            &self.runtime.registry,
            context,
            &live,
        )?;
        let ingress = self
            .runtime
            .access
            .ingress_for_admission(
                &self.runtime.owner.lease(),
                seen.client,
                device,
                context.client_id,
            )
            .map_err(|_| PrivateInputIssueRefusal::ConnectionGone)?;
        Ok(PrivateInputSubmission::new(
            Arc::clone(&self.runtime),
            ingress,
            PrivateInputConnection {
                client: seen.client,
                admission: seen.admission,
                connection_generation: seen.connection_generation,
            },
            device,
        ))
    }

    /// Revoke one admission and retire exactly the grants it authorised.
    pub fn revoke(
        &self,
        context: ClientAdmissionContext,
    ) -> Result<PrivateInputConnection, PrivateInputIssueRefusal> {
        let live = self
            .runtime
            .participant
            .admitted()
            .map_err(|_| PrivateInputIssueRefusal::Unavailable)?;
        let seen = live
            .iter()
            .find(|seen| seen.admission == context.client_id)
            .ok_or(PrivateInputIssueRefusal::ConnectionGone)?;
        self.runtime
            .participant
            .revoke_admission(seen.client, context.client_id)
            .map_err(|_| PrivateInputIssueRefusal::ConnectionGone)?;
        Ok(PrivateInputConnection {
            client: seen.client,
            admission: seen.admission,
            connection_generation: seen.connection_generation,
        })
    }

    /// Take the delivery receipts that have arrived, without waiting.
    ///
    /// DRAINED, NEVER PROBED AWAY. Each call returns what is queued and leaves
    /// nothing behind; asking whether anything is there is the same act as
    /// taking it, so there is no separate question that consumes.
    pub fn drain_deliveries(&self) -> Result<PrivateInputReceipts, PrivateInputUnavailable> {
        self.consume_receipts(None)
    }

    /// Take delivery receipts, waiting up to this bound for the first one.
    pub fn drain_deliveries_within(
        &self,
        within: Duration,
    ) -> Result<PrivateInputReceipts, PrivateInputUnavailable> {
        self.consume_receipts(Some(within))
    }

    /// Take receipts and hand each one back to the ledger that issued it.
    ///
    /// A RECEIPT IS NOT CONSUMED BY BEING READ. A delivery's ticket is pruned
    /// only once it is both routing-finished and observed, so a receipt that is
    /// popped off the channel and merely handed to a caller leaves its place
    /// taken for good. Enough of those and the service stops accepting
    /// deliveries without having grown by a byte: the bound is on tickets, not
    /// on the channel. Observing here is what gives the place back, and it
    /// reuses the ledger's own ticket rather than counting anything twice.
    ///
    /// AN UNOBSERVED RECEIPT IS KEPT. The ledger refuses an observation whose
    /// ticket it does not hold, one already made, and one whose terminal
    /// answer is not the receipt offered. None of those is a reason to drop the
    /// receipt: it is retained, offered again on the next drain, and counted as
    /// owed at stop.
    fn consume_receipts(
        &self,
        within: Option<Duration>,
    ) -> Result<PrivateInputReceipts, PrivateInputUnavailable> {
        // EVERY FALLIBLE ACQUISITION BEFORE ANY OBSERVATION. An earlier version
        // observed the retained receipts first and then reached for the
        // delivery channel, so a poisoned channel returned `Err` and took the
        // already-observed receipts with it: their places were given back in
        // the ledger and the caller was never told which receipts those were.
        // Both locks are taken here, and after that nothing can fail.
        let mut retained = self
            .runtime
            .retained_receipts
            .lock()
            .map_err(|_| PrivateInputUnavailable)?;
        let held = self
            .runtime
            .deliveries
            .lock()
            .map_err(|_| PrivateInputUnavailable)?;

        // TAKEN INTO CUSTODY BEFORE ANYTHING IS OBSERVED. A receipt moved out
        // of the channel and into this queue has changed owner and nothing
        // else; it is not consumed, not observed, and still owed.
        let room = PRIVATE_INPUT_RECEIPT_RETENTION.saturating_sub(retained.len());
        if room > 0 {
            let mut taken = Vec::new();
            if let Some(within) = within
                && let Ok(first) = held.recv_timeout(within)
            {
                taken.push(first);
            }
            taken.extend(held.try_iter().take(room - taken.len()));
            retained.extend(taken);
        }
        drop(held);

        // BOUNDED BY WHAT THIS CALL VISITS, AND VISITED IN PLACE. Counting the
        // remainder let a full queue be observed and then a full channel
        // drained on top, which is twice the bound this advertises. Walking the
        // whole queue to rebuild it would also make the work proportional to
        // everything held rather than to the bound, so only the prefix is
        // touched and only released receipts are removed.
        let mut observed = Vec::new();
        let mut unreadable = 0usize;
        let limit = retained.len().min(PRIVATE_INPUT_DRAIN_BOUND);
        let mut visited = 0usize;
        let mut index = 0usize;
        while visited < limit {
            let receipt = retained[index];
            visited += 1;
            match self.runtime.observer.observe(receipt) {
                sophia_x_authority::PrivateDeliveryObservation::Observed => {
                    retained.remove(index);
                    observed.push(receipt);
                }
                sophia_x_authority::PrivateDeliveryObservation::Unreadable => {
                    // NOT A REFUSAL. The ledger was never asked, so this is
                    // counted apart from the receipts it actually declined.
                    unreadable += 1;
                    index += 1;
                }
                _ => index += 1,
            }
        }

        Ok(PrivateInputReceipts {
            observed,
            retained: retained.len(),
            visited,
            unreadable,
        })
    }

    /// Take queued receipts into custody and report what is owed.
    ///
    /// IT MOVES RECEIPTS, AND IT HAS TO. A count that read only the retained
    /// queue reported zero while receipts sat unread in the delivery channel,
    /// each one still holding its ticket's place; the channel cannot be
    /// measured without taking from it. Nothing here is observed, so nothing
    /// is consumed: the receipts change owner from the channel to this
    /// service, which is where they were going to have to be counted anyway.
    ///
    /// IT SAYS WHEN IT COULD NOT TAKE EVERYTHING. The retention bound can stop
    /// it short of emptying the channel, and a total reported in that case
    /// would be a total of what fitted rather than of what is owed. The answer
    /// carries whether the channel was actually emptied, so a partial reading
    /// can never be mistaken for a complete one.
    pub fn unobserved_receipts(
        &self,
    ) -> Result<PrivateInputReceiptInventory, PrivateInputUnavailable> {
        let mut retained = self
            .runtime
            .retained_receipts
            .lock()
            .map_err(|_| PrivateInputUnavailable)?;
        let held = self
            .runtime
            .deliveries
            .lock()
            .map_err(|_| PrivateInputUnavailable)?;
        let room = PRIVATE_INPUT_RECEIPT_RETENTION.saturating_sub(retained.len());
        let taken: Vec<_> = held.try_iter().take(room).collect();
        // Fewer than there was room for means the channel ran out, which is the
        // only way to know it is empty. Taking exactly the room available
        // leaves the question open, and so does having no room at all.
        let complete = taken.len() < room;
        retained.extend(taken);
        Ok(PrivateInputReceiptInventory {
            retained: retained.len(),
            complete,
        })
    }

    /// Take the control acknowledgements that have arrived, without waiting.
    ///
    /// DRAINED, NEVER PROBED AWAY. Each call returns what is queued and leaves
    /// nothing behind; asking whether anything is there is the same act as
    /// taking it, so there is no separate question that consumes.
    pub fn drain_acknowledgements(&self) -> Vec<XAuthorityClientControlAck> {
        self.runtime
            .acknowledgements
            .lock()
            .map(|held| held.try_iter().take(PRIVATE_INPUT_DRAIN_BOUND).collect())
            .unwrap_or_default()
    }

    /// Take control acknowledgements, waiting up to this bound for the first one.
    pub fn drain_acknowledgements_within(
        &self,
        within: Duration,
    ) -> Vec<XAuthorityClientControlAck> {
        let Ok(held) = self.runtime.acknowledgements.lock() else {
            return Vec::new();
        };
        let mut taken = Vec::new();
        if let Ok(first) = held.recv_timeout(within) {
            taken.push(first);
        }
        taken.extend(
            held.try_iter()
                .take(PRIVATE_INPUT_DRAIN_BOUND - taken.len()),
        );
        taken
    }

    /// Take the observed transaction batches that have arrived, without waiting.
    ///
    /// DRAINED, NEVER PROBED AWAY. Each call returns what is queued and leaves
    /// nothing behind; asking whether anything is there is the same act as
    /// taking it, so there is no separate question that consumes.
    pub fn drain_transactions(&self) -> Vec<XAuthorityObservedTransactionBatch> {
        self.runtime
            .transactions
            .lock()
            .map(|held| held.try_iter().take(PRIVATE_INPUT_DRAIN_BOUND).collect())
            .unwrap_or_default()
    }

    /// Take observed transaction batches, waiting up to this bound for the first one.
    pub fn drain_transactions_within(
        &self,
        within: Duration,
    ) -> Vec<XAuthorityObservedTransactionBatch> {
        self.drain_transactions_limited(within, PRIVATE_INPUT_DRAIN_BOUND)
    }

    /// Take at most `limit` batches, however long the drain bound is.
    ///
    /// THE CALLER'S REMAINING ROOM IS THE LIMIT THAT MATTERS. Asking whether
    /// there was any room and then taking a whole drain bound's worth is how a
    /// bounded queue reaches half again its bound: room for one is not room for
    /// two hundred and fifty six.
    pub fn drain_transactions_limited(
        &self,
        within: Duration,
        limit: usize,
    ) -> Vec<XAuthorityObservedTransactionBatch> {
        let limit = limit.min(PRIVATE_INPUT_DRAIN_BOUND);
        if limit == 0 {
            return Vec::new();
        }
        let Ok(held) = self.runtime.transactions.lock() else {
            return Vec::new();
        };
        let mut taken = Vec::new();
        if let Ok(first) = held.recv_timeout(within) {
            taken.push(first);
        }
        taken.extend(held.try_iter().take(limit - taken.len()));
        taken
    }

    pub fn submit_action(
        &self,
        connection: PrivateInputConnection,
        action: PrivateInputAction,
    ) -> Result<PrivateInputSubmitted, PrivateInputControlError> {
        // THE WHOLE IDENTITY IS CHECKED, NOT JUST THE CLIENT. A client number
        // is reused by a successor connection, so matching on it alone would
        // let a control minted against a connection that has since ended be
        // served to whoever now holds that number. The admission and the
        // connection generation are what make this connection this one, and a
        // stale connection is refused before any identity is spent on it.
        let live = self
            .runtime
            .participant
            .admitted()
            .map_err(|_| PrivateInputControlError::Unavailable)?;
        if !live.iter().any(|seen| {
            seen.client == connection.client
                && seen.admission == connection.admission
                && seen.connection_generation == connection.connection_generation
                && !seen.closed
        }) {
            return Err(PrivateInputControlError::ConnectionGone);
        }
        // Exhaustion is refused before the command is built, so no control is
        // ever submitted under an identity another one already holds.
        let transaction = self
            .runtime
            .next_transaction()
            .ok_or(PrivateInputControlError::Exhausted)?;
        let surface = action.surface();
        let command = sophia_x_authority::XAuthorityClientControlCommand {
            client: connection.client,
            command: match action {
                PrivateInputAction::FocusSurface { surface } => {
                    sophia_x_authority::XAuthorityControlCommand::FocusSurface {
                        transaction,
                        surface,
                    }
                }
                PrivateInputAction::ClearFocus { surface } => {
                    sophia_x_authority::XAuthorityControlCommand::ClearFocus {
                        transaction,
                        surface,
                    }
                }
            },
        };
        let producer = self
            .runtime
            .access
            .control_producer(&self.runtime.owner.lease())
            .map_err(|_| PrivateInputControlError::Ended)?;
        producer
            .submit(&self.runtime.owner.lease(), command)
            .map(|_sequence| PrivateInputSubmitted {
                transaction,
                surface,
                kind: action.kind(),
            })
            .map_err(|(refusal, command)| PrivateInputControlError::Refused(refusal, command))
    }

    /// Stop the service and collect it.
    ///
    /// Producer admission closes first, then the invocation is interrupted and
    /// its actors collected, and only then does the maintenance this keeper is
    /// still allowed to perform run. The keeper stays on the thread it was
    /// made on throughout.
    pub fn stop(self) -> PrivateInputOutcome {
        let runtime = Arc::clone(&self.runtime);
        runtime
            .stop_once()
            .unwrap_or_default()
            .with_retention(runtime)
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
    fn drop(&mut self) {
        // THE CONTROLLER STOPS, WHOEVER ELSE STILL HOLDS THE RUNTIME. An
        // earlier version stopped only when this was the last `Arc`, which
        // meant any live `PrivateInputSubmission` -- an adapter holding exactly
        // the custody this design hands out -- silently turned a drop into no
        // stop at all. The runtime then dropped its `JoinHandle` without
        // joining, leaving the service thread running with nobody to answer
        // for it. Adapters keep runtime custody; they are not controllers, and
        // their custody is not a veto on stopping.
        //
        // Stopping is idempotent, so an explicit `stop` followed by this drop
        // joins once. The report is discarded rather than the stop skipped,
        // and anything still owed stays owned by the durable store this
        // runtime keeps, which outlives the facade.
        // AND THE RETENTION IS APPLIED HERE TOO. An earlier version performed
        // the stop and discarded its outcome without ever running retention,
        // so the reserved closing slot was never filled on the one path it
        // exists for -- a controller that goes without an explicit stop -- and
        // a clean drop never gave its claim back either. The report is what is
        // discarded; the custody decision is not.
        if let Some(outcome) = self.runtime.stop_once() {
            let _ = outcome.with_retention(Arc::clone(&self.runtime));
        }
    }
}
