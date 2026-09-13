// Per-operation completion records for control on the private path.
//
// A control command ends in more ways than it is acknowledged. It can be
// queued and never dequeued, fail partway with the runtime already mutated,
// be overtaken by a client termination, or have its writer torn down before it
// reaches an acknowledgement at all. Each of those is a real ending and each
// needs an owner, so every accepted control gets a record here before it is
// accepted, and that record is what answers for it afterwards.

/// An opaque, server-issued registration for one control operation.
///
/// Not a client's transaction id. An acknowledgement names only a client and a
/// public transaction, and those alias across requests that share a
/// transaction, so a mapping built from them would answer one request with
/// another's outcome.
///
/// Neither part is ever reused: a registry that cannot allocate a fresh origin
/// is not built, and a registry that cannot advance its counter refuses to
/// register rather than issue a value it has already issued.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlCompletionToken {
    /// Which registry issued it. Two instances never share a value.
    origin: u64,
    /// Which operation within that registry.
    incarnation: u64,
}

/// What an operation was registered as.
///
/// Immutable, and kept apart from the phase. Everything that later claims to
/// be about this operation is checked against it, so an acknowledgement for
/// one request cannot settle another that happens to hold the token.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ControlOperationIdentity {
    client: XServerFrontendClientId,
    kind: XAuthorityControlKind,
    transaction: TransactionId,
    surface: SurfaceId,
}

#[cfg(unix)]
impl ControlOperationIdentity {
    fn of(command: &XAuthorityClientControlCommand) -> Self {
        Self {
            client: command.client,
            kind: command.command.kind(),
            transaction: command.command.transaction(),
            surface: command.command.surface(),
        }
    }

    /// Whether an acknowledgement is this operation's, outcome aside.
    ///
    /// The outcome is what an acknowledgement is free to carry; everything
    /// naming which operation it belongs to must match what was registered.
    fn answers(&self, acknowledgement: &XAuthorityClientControlAck) -> bool {
        self.client == acknowledgement.client
            && self.kind == acknowledgement.acknowledgement.kind
            && self.transaction == acknowledgement.acknowledgement.transaction
            && self.surface == acknowledgement.acknowledgement.surface
    }
}

/// How far one control operation has got.
///
/// What is retained differs by phase, and collapsing the three loses the case
/// that matters most.
#[cfg(unix)]
enum ControlPhase {
    /// Storage is taken and the work still belongs to its producer.
    ///
    /// Reserving before acceptance is what lets a refusal hand the command
    /// back, but a reservation is not acceptance: nothing here has been
    /// promised a consumer, and nothing here may answer for it. The producer
    /// either hands it over or takes it back.
    Reserved(XAuthorityClientControlCommand),
    /// Accepted and not executed. The command itself is kept, because it can
    /// still be executed or cancelled and there is no outcome yet.
    Accepted(XAuthorityClientControlCommand),
    /// Execution has been claimed and no outcome is established. The runtime
    /// may already have mutated, so this is neither unexecuted nor answered;
    /// it stays this way until something establishes what happened. Inventing
    /// an outcome to leave this state would be fabricating a receipt.
    ///
    /// The command is kept for the identity and cleanup responsibility it
    /// carries, not to be replayed: replaying a partly applied command is the
    /// mistake this phase exists to prevent.
    Applying(XAuthorityClientControlCommand),
    /// Execution began and the executor that could have established an outcome
    /// has gone. No outcome is coming.
    ///
    /// Not a terminal outcome, and nothing is published for it: what the
    /// runtime did is still unknown, and saying otherwise would invent the
    /// receipt every other rule here exists to avoid inventing. What it does
    /// establish is that the cleanup this operation named is now owed to
    /// someone, and owed until it is recorded done -- so the credit stays held
    /// and the record stays outstanding.
    Abandoned(XAuthorityClientControlCommand),
    /// Its outcome has been published, and it is held only because work it
    /// queued elsewhere can still run.
    ///
    /// Nothing further is published for it -- the acknowledgement went out
    /// once and the last dependency ending is not a reason to send it again.
    /// What survives is the obligation and the storage reserved for it, so the
    /// credit is not freed while an effect this operation started is still
    /// able to happen.
    Settled(XServerFrontendClientId),
    /// An outcome is established and its acknowledgement has not been
    /// published. The effect has happened, so this is republished, never
    /// replayed, and never replaced: the first established outcome is what
    /// happened, and a later contradicting one is a bug in the caller rather
    /// than a correction.
    Owed(XAuthorityClientControlAck),
}

#[cfg(unix)]
impl ControlPhase {
    /// The client this operation belongs to, in every phase.
    ///
    /// An owed acknowledgement names its client as surely as an unexecuted
    /// command does, so nothing is exempt from a per-client edge.
    fn client(&self) -> XServerFrontendClientId {
        match self {
            Self::Reserved(command)
            | Self::Accepted(command)
            | Self::Applying(command)
            | Self::Abandoned(command) => command.client,
            Self::Settled(client) => *client,
            Self::Owed(acknowledgement) => acknowledgement.client,
        }
    }
}

#[cfg(unix)]
struct ControlRecord {
    token: ControlCompletionToken,
    /// What this operation has actually done, as it reported it.
    steps: ControlSteps,
    /// Effects this operation queued on someone else, not yet ended.
    ///
    /// Routing a focus change puts a FocusOut on the previously focused
    /// client's writer queue. That entry outlives the operation that caused
    /// it, and this operation's own router and writer going quiet says nothing
    /// about whether it has run. Counted here so that nothing about this
    /// operation is settled while work it started can still happen.
    dependents: usize,
    identity: ControlOperationIdentity,
    phase: ControlPhase,
}

/// Why a completion could not be registered.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlCompletionRefusal {
    /// No record storage remains. Retrying later is sensible.
    AtCapacity,
    /// The registry cannot be reached.
    Unavailable,
    /// Registrations are exhausted. Terminal: the next identity would repeat
    /// one already issued, and two operations sharing an identity is the one
    /// thing this record cannot survive.
    Exhausted,
}

/// What publishing an acknowledgement achieved.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlPublication {
    /// The acknowledgement reached the receiver.
    Delivered,
    /// The channel is full. The acknowledgement is retained for publication,
    /// and the command is not replayed: its effect has already happened.
    Retained,
    /// The receiver is gone, so nothing was published. Distinct from delivery,
    /// and distinct from the client having disappeared.
    ReceiverGone,
}

/// Why an acknowledgement was not recorded against a registration.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlPublicationRefusal {
    /// No record is held for it. Something else owns its outcome now, and
    /// resurrecting it here would answer for work this registry gave up.
    NoLongerHeld,
    /// This registry did not issue the registration.
    Foreign,
    /// The acknowledgement names a different operation from the one
    /// registered under this token.
    NotThisOperation,
    /// An outcome is already established and this one contradicts it. The
    /// first is what happened.
    OutcomeAlreadyEstablished,
    /// Its outcome has already been published. The record survives only for
    /// work it queued elsewhere, and nothing further is sent for it.
    AlreadyPublished,
    /// The registry cannot be reached.
    Unavailable,
    /// The work is still its producer's; it has not been accepted, so no
    /// outcome may be published for it.
    NotAccepted,
    /// Its executor has gone with the outcome unestablished. Nothing here
    /// knows what happened, so nothing here may say.
    Abandoned,
}

/// Why an execution claim was refused.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlClaimRefusal {
    /// No record is held for it: it was settled or cancelled, and something
    /// else owns its outcome and cleanup.
    NoLongerHeld,
    /// This registry did not issue the registration.
    Foreign,
    /// Execution is already claimed. Starting it again would apply it twice.
    AlreadyApplying,
    /// Execution was never claimed, so this is not a continuation of it. The
    /// first authoritative effect was skipped.
    NotStarted,
    /// The work is still its producer's; it has not been accepted.
    NotAccepted,
    /// Its outcome is established. Applying it now would act after the answer.
    AlreadyAnswered,
    /// The registry cannot be reached, so nothing can be established.
    Unavailable,
    /// Execution began and the executor that could have established an outcome
    /// has gone. Its cleanup is owed; its application is not to be resumed.
    Abandoned,
    /// Nothing is executing for this client, so nothing may begin.
    NoExecutor,
}

/// Whether a caller may produce this operation's effects.
///
/// A refusal is not advice. Every variant that is not permission means some
/// other owner holds this operation's outcome and cleanup, so producing an
/// effect or an acknowledgement here would be acting after, or beside, an
/// answer that already belongs somewhere else.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use = "a refused claim must prevent the effect it was asked about"]
pub enum ControlExecutionClaim {
    /// No completion record governs this operation.
    Ungoverned,
    /// Execution is now claimed by this caller.
    Claimed,
    /// Execution was already claimed and this caller is its continuation.
    Resumed,
    /// This caller may not produce effects for it.
    Refused(ControlClaimRefusal),
}

#[cfg(unix)]
impl ControlExecutionClaim {
    /// Whether the caller may go on to produce this operation's effects.
    pub fn permits_effects(self) -> bool {
        matches!(self, Self::Ungoverned | Self::Claimed | Self::Resumed)
    }
}

#[cfg(unix)]
struct ControlCompletions {
    records: Vec<ControlRecord>,
    /// How many executors are live for each client.
    ///
    /// A writer is one. So is a routing call in flight, because routing is
    /// where the first authoritative effect happens -- focus routing sends
    /// FocusOut and moves the focused surface before any writer runs, which is
    /// why the claim lives there. Counting only writers would let a writer's
    /// exit abandon an operation another thread is still inside.
    ///
    /// Held here so that claiming and abandoning contend for one lock. A
    /// caller that checked liveness elsewhere and then claimed would have a
    /// gap between the two, and a sweep landing in it turns an untouched
    /// record into an applying one after the sweep has passed.
    executors: BTreeMap<XServerFrontendClientId, ControlExecutors>,
    next_incarnation: u64,
    capacity: usize,
}

/// The server-owned registry of control completions for one private instance.
#[cfg(unix)]
#[derive(Clone)]
pub struct ControlCompletionRegistry {
    origin: u64,
    inner: Arc<Mutex<ControlCompletions>>,
}

#[cfg(unix)]
static CONTROL_COMPLETION_ORIGINS: AtomicU64 = AtomicU64::new(1);

#[cfg(unix)]
impl ControlCompletionRegistry {
    /// Build a registry, or refuse because no unused origin remains.
    ///
    /// Fallible because the alternative is a second registry sharing an
    /// origin with a live one, and then one instance's token would name an
    /// operation in another's records.
    pub fn with_capacity(capacity: usize) -> Option<Self> {
        let origin = CONTROL_COMPLETION_ORIGINS
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                current.checked_add(1)
            })
            .ok()?;
        Some(Self {
            origin,
            inner: Arc::new(Mutex::new(ControlCompletions {
                records: Vec::with_capacity(capacity),
                executors: BTreeMap::new(),
                next_incarnation: 1,
                capacity,
            })),
        })
    }

    /// Register before the work is accepted.
    ///
    /// Taking the storage first is what makes every later transition
    /// possible: a record that could be refused after acceptance would leave
    /// an accepted command with no owner. Every refusal hands the command
    /// back, and says which refusal it was.
    pub fn register(
        &self,
        command: XAuthorityClientControlCommand,
    ) -> Result<ControlCompletionToken, (ControlCompletionRefusal, XAuthorityClientControlCommand)>
    {
        let Ok(mut inner) = self.inner.lock() else {
            return Err((ControlCompletionRefusal::Unavailable, command));
        };
        if inner.records.len() >= inner.capacity {
            return Err((ControlCompletionRefusal::AtCapacity, command));
        }
        // Advanced before the identity is issued, so the value handed out is
        // one this registry can prove it will never hand out again.
        let Some(next) = inner.next_incarnation.checked_add(1) else {
            return Err((ControlCompletionRefusal::Exhausted, command));
        };
        let token = ControlCompletionToken {
            origin: self.origin,
            incarnation: inner.next_incarnation,
        };
        inner.next_incarnation = next;
        inner.records.push(ControlRecord {
            token,
            steps: ControlSteps::default(),
            dependents: 0,
            identity: ControlOperationIdentity::of(&command),
            phase: ControlPhase::Reserved(command),
        });
        Ok(token)
    }

    /// Give up a reservation the producer is taking back.
    ///
    /// Distinct from discarding an accepted command: nothing was ever handed
    /// over, so there is no second owner and nothing to answer.
    pub fn release_reservation(&self, token: ControlCompletionToken) -> bool {
        if token.origin != self.origin {
            return false;
        }
        let Ok(mut inner) = self.inner.lock() else {
            return false;
        };
        let mut released = false;
        inner.records.retain(|held| {
            if held.token != token || !matches!(held.phase, ControlPhase::Reserved(_)) {
                return true;
            }
            released = true;
            false
        });
        released
    }

    /// Begin taking a reservation into acceptance.
    ///
    /// The returned handle holds the registry, so the caller can publish its
    /// queue entry and commit the phase as one transaction. Committed
    /// together, or rolled back together: an acceptance recorded beside the
    /// publication can leave the instance owning queued work whose record
    /// still says its producer owns it, and a promotion whose result nobody
    /// checks leaves exactly that.
    ///
    /// `None` when there is nothing to hand over: a foreign token, an
    /// unreachable registry, a record already gone, or one not in reserve.
    pub fn begin_acceptance(&self, token: ControlCompletionToken) -> Option<ControlAcceptance<'_>> {
        if token.origin != self.origin {
            return None;
        }
        let mut inner = self.inner.lock().ok()?;
        let position = inner.records.iter().position(|held| held.token == token)?;
        let ControlPhase::Reserved(command) = inner.records[position].phase else {
            return None;
        };
        inner.records[position].phase = ControlPhase::Accepted(command);
        Some(ControlAcceptance {
            held: Some((inner, position)),
        })
    }

    /// Claim execution before the first authoritative effect.
    ///
    /// Not a notification. The caller is asking whether it may make this
    /// operation happen, and a refusal means some other owner already holds
    /// its outcome and cleanup. Claiming and cancelling contend for the same
    /// lock, so an operation is either claimed and never reported unexecuted,
    /// or cancelled and never applied.
    pub fn claim_execution(&self, token: ControlCompletionToken) -> ControlExecutionClaim {
        self.transition_claim(token, true)
    }

    /// Continue an operation whose execution is already claimed.
    ///
    /// A writer is a continuation, not a beginning: the routing that put the
    /// command in its queue was already an authoritative effect. Finding it
    /// unclaimed means that ordering was skipped, which is refused rather
    /// than repaired by claiming it late.
    pub fn resume_execution(&self, token: ControlCompletionToken) -> ControlExecutionClaim {
        self.transition_claim(token, false)
    }

    fn transition_claim(
        &self,
        token: ControlCompletionToken,
        starting: bool,
    ) -> ControlExecutionClaim {
        if token.origin != self.origin {
            return ControlExecutionClaim::Refused(ControlClaimRefusal::Foreign);
        }
        let Ok(mut inner) = self.inner.lock() else {
            return ControlExecutionClaim::Refused(ControlClaimRefusal::Unavailable);
        };
        let Some(position) = inner.records.iter().position(|held| held.token == token) else {
            return ControlExecutionClaim::Refused(ControlClaimRefusal::NoLongerHeld);
        };
        // What the record itself is comes first. A reservation is not the
        // instance's at all, and an abandoned operation stays abandoned
        // however many executors appear afterwards.
        if matches!(inner.records[position].phase, ControlPhase::Reserved(_)) {
            return ControlExecutionClaim::Refused(ControlClaimRefusal::NotAccepted);
        }
        if matches!(inner.records[position].phase, ControlPhase::Abandoned(_)) {
            return ControlExecutionClaim::Refused(ControlClaimRefusal::Abandoned);
        }
        // Under the same lock that abandons, so a claim and a sweep cannot
        // both decide they were first.
        //
        // Starting asks a stricter question than continuing. An owner already
        // inside an operation keeps that operation answerable, and is not
        // permission to begin another: borrowing its in-flight existence would
        // start work for a client whose writer has gone.
        let client = inner.records[position].phase.client();
        let executing = if starting {
            Self::admits(&inner, client)
        } else {
            Self::executing(&inner, client)
        };
        if !executing {
            return ControlExecutionClaim::Refused(ControlClaimRefusal::NoExecutor);
        }
        let record = &mut inner.records[position];
        match (&record.phase, starting) {
            // Not accepted, so no part of the instance may act on it yet.
            (ControlPhase::Reserved(_), _) => {
                ControlExecutionClaim::Refused(ControlClaimRefusal::NotAccepted)
            }
            (ControlPhase::Accepted(command), true) => {
                record.phase = ControlPhase::Applying(*command);
                ControlExecutionClaim::Claimed
            }
            (ControlPhase::Accepted(_), false) => {
                ControlExecutionClaim::Refused(ControlClaimRefusal::NotStarted)
            }
            (ControlPhase::Applying(_), true) => {
                ControlExecutionClaim::Refused(ControlClaimRefusal::AlreadyApplying)
            }
            (ControlPhase::Applying(_), false) => ControlExecutionClaim::Resumed,
            (ControlPhase::Owed(_) | ControlPhase::Settled(_), _) => {
                ControlExecutionClaim::Refused(ControlClaimRefusal::AlreadyAnswered)
            }
            // The executor that was applying it has gone. Picking it up now
            // would apply an operation whose earlier application nobody can
            // describe, on top of whatever that left behind.
            (ControlPhase::Abandoned(_), _) => {
                ControlExecutionClaim::Refused(ControlClaimRefusal::Abandoned)
            }
        }
    }

    /// How many operations still have an unanswered record, or `None` if the
    /// registry could not be read.
    ///
    /// Not zero on failure. Nothing outstanding and nothing knowable are
    /// different answers, and a caller told the first when the second is true
    /// walks away from work that is still owed.
    pub fn outstanding(&self) -> Option<usize> {
        self.inner.lock().ok().map(|inner| inner.records.len())
    }

    /// How many are holding an acknowledgement that could not be published,
    /// or `None` if the registry could not be read.
    pub fn owed(&self) -> Option<usize> {
        self.inner.lock().ok().map(|inner| {
            inner
                .records
                .iter()
                .filter(|held| matches!(held.phase, ControlPhase::Owed(_)))
                .count()
        })
    }

    /// Retry the acknowledgements that could not be published.
    ///
    /// Only the outcomes are offered, never the commands: republishing an
    /// acknowledgement is right, and re-running a command whose effect already
    /// happened is not.
    ///
    /// Each is retired exactly when the caller reports it delivered, and stays
    /// otherwise. Handing the acknowledgements out and expecting them back
    /// would give the caller something it could drop, and a dropped outcome is
    /// the failure this record exists to prevent.
    pub fn publish_owed_with(
        &self,
        mut publish: impl FnMut(&XAuthorityClientControlAck) -> ControlPublication,
    ) -> usize {
        let Ok(mut inner) = self.inner.lock() else {
            return 0;
        };
        let mut delivered = 0usize;
        inner.records.retain_mut(|held| {
            let ControlPhase::Owed(acknowledgement) = held.phase else {
                return true;
            };
            match publish(&acknowledgement) {
                ControlPublication::Delivered => {
                    delivered = delivered.saturating_add(1);
                    // Same rule as publishing directly: answered is not over.
                    // Settled where it stands, so a recovery path allocates
                    // nothing after publishing and leaves no phase to fix up
                    // in a second pass.
                    if held.dependents == 0 {
                        false
                    } else {
                        held.phase = ControlPhase::Settled(acknowledgement.client);
                        true
                    }
                }
                ControlPublication::Retained | ControlPublication::ReceiverGone => true,
            }
        });
        delivered
    }

    /// What became of one registration.
    ///
    /// Absence only means retirement for a token this registry issued. A
    /// foreign token is absent too, and treating that as an outcome would
    /// answer for work this registry never accepted.
    pub fn state_of(&self, token: ControlCompletionToken) -> ControlRecordState {
        if token.origin != self.origin {
            return ControlRecordState::Unanswerable;
        }
        let Ok(inner) = self.inner.lock() else {
            return ControlRecordState::Unanswerable;
        };
        if inner.records.iter().any(|held| held.token == token) {
            ControlRecordState::Outstanding
        } else {
            // Origins are allocated without reuse and clones share one, so a
            // matching origin means this registry issued the token. Having
            // issued it and no longer holding it leaves only one reading: it
            // was settled.
            ControlRecordState::Retired
        }
    }

    /// Move one client's applying operations to abandoned.
    ///
    /// Called where nothing is left that could establish an outcome for them.
    /// Nothing is published and nothing is replayed: what each names now is
    /// the cleanup it is owed.
    fn abandon_unexecutable(inner: &mut ControlCompletions, client: XServerFrontendClientId) {
        for held in inner.records.iter_mut() {
            if held.phase.client() != client {
                continue;
            }
            if let ControlPhase::Applying(command) = held.phase {
                held.phase = ControlPhase::Abandoned(command);
            }
        }
    }

    /// Reconcile one client's records when its registration goes.
    ///
    /// Three different things, kept apart, because collapsing them is how a
    /// receipt gets invented:
    ///
    /// - an established outcome stays exactly as it is, still to publish;
    /// - a command that never started stays too, still truthfully unexecuted,
    ///   and is handed on when the instance closes rather than from here;
    /// - a command caught mid-application becomes abandoned. Nothing is
    ///   published for it and nothing is replayed. What its registration now
    ///   names is the cleanup it is owed, and the credit stays with it until
    ///   that cleanup is recorded done.
    ///
    /// Reservations are left alone: they are still their producer's.
    ///
    /// An operation is abandoned only where nothing is executing for its
    /// client, and that is read here under the same lock rather than asserted
    /// by the caller. A writer is not the whole executor: routing is where the
    /// first authoritative effect happens, so a routing call in flight is one
    /// too, and a writer's exit while a router is inside an operation it
    /// already claimed must not abandon it. Abandoning one that is still being
    /// applied would take an operation with a live executor and an outcome
    /// about to be established and turn it into one that can never be
    /// answered.
    ///
    /// Counts, not payloads. This runs on a teardown path, and a caller that
    /// wanted the abandoned operations themselves asks for them separately
    /// rather than having a vector built for it on the way out.
    pub fn reconcile_client(&self, client: XServerFrontendClientId) -> ControlReconciliation {
        let Ok(mut inner) = self.inner.lock() else {
            return ControlReconciliation::unavailable();
        };
        let executor_gone = !Self::executing(&inner, client);
        let mut reconciled = ControlReconciliation {
            readable: true,
            ..ControlReconciliation::default()
        };
        for held in inner.records.iter_mut() {
            if held.phase.client() != client {
                continue;
            }
            match held.phase {
                ControlPhase::Reserved(_) => {
                    reconciled.reserved = reconciled.reserved.saturating_add(1);
                }
                ControlPhase::Accepted(_) => {
                    reconciled.unexecuted = reconciled.unexecuted.saturating_add(1);
                }
                ControlPhase::Applying(command) => {
                    if executor_gone {
                        held.phase = ControlPhase::Abandoned(command);
                        reconciled.abandoned = reconciled.abandoned.saturating_add(1);
                    } else {
                        reconciled.applying = reconciled.applying.saturating_add(1);
                    }
                    let _ = command;
                }
                ControlPhase::Abandoned(_) => {
                    reconciled.abandoned = reconciled.abandoned.saturating_add(1);
                }
                ControlPhase::Owed(_) | ControlPhase::Settled(_) => {
                    reconciled.owed = reconciled.owed.saturating_add(1);
                }
            }
        }
        reconciled
    }

    /// The operations whose cleanup is owed, with what each named.
    ///
    /// For an owner that can actually perform the cleanup. The records stay
    /// here: this is what is owed, not a handover, and each is retired only
    /// when the cleanup is recorded done.
    ///
    /// A registry that cannot be read says so rather than returning nothing.
    /// An empty list means nothing is owed; it must never also mean nobody
    /// could look.
    pub fn cleanups_owed(&self) -> Result<Vec<ControlCleanup>, ControlCleanupRefusal> {
        let Ok(inner) = self.inner.lock() else {
            return Err(ControlCleanupRefusal::Unavailable);
        };
        Ok(inner
            .records
            .iter()
            .filter_map(|held| match held.phase {
                // Abandoned, and nothing it queued elsewhere can still run.
                // While one can, this operation is not waiting on a cleanup:
                // it is waiting to find out what else it did.
                ControlPhase::Abandoned(command) if held.dependents == 0 => Some(ControlCleanup {
                    token: held.token,
                    command,
                    steps: held.steps,
                }),
                _ => None,
            })
            .collect())
    }

    /// Give up this record because the operation now has another owner.
    ///
    /// Only an unexecuted command can be handed on, so only that phase is
    /// discarded here. An applying operation still needs its cleanup
    /// responsibility recorded, and an owed acknowledgement is the only copy
    /// of an outcome that already happened -- discarding either would lose
    /// what the record exists to hold.
    pub fn discard(&self, token: ControlCompletionToken) -> bool {
        if token.origin != self.origin {
            return false;
        }
        let Ok(mut inner) = self.inner.lock() else {
            return false;
        };
        let mut discarded = false;
        inner.records.retain(|held| {
            if held.token != token || !matches!(held.phase, ControlPhase::Accepted(_)) {
                return true;
            }
            discarded = true;
            false
        });
        discarded
    }

    /// Settle every record that has not reached an outcome, at a cancellation
    /// edge.
    ///
    /// Returns the commands that had not claimed execution, with the
    /// registration each was held under, so the caller can transfer ownership
    /// exactly rather than by a transaction that aliases. Those that had
    /// claimed it are counted separately and retained. A command caught
    /// mid-application is not reported as unexecuted: the runtime may already
    /// have changed, and saying otherwise would be inventing the outcome this
    /// whole record exists to avoid inventing.
    pub fn cancel_unfinished(&self) -> ControlCancellation {
        let Ok(mut inner) = self.inner.lock() else {
            return ControlCancellation::default();
        };
        let mut cancellable = Vec::new();
        let mut indeterminate = 0usize;
        let mut reserved = 0usize;
        inner.records.retain(|held| match held.phase {
            // Not this instance's to cancel. A reservation is its producer's
            // until acceptance, and answering for one would let the producer
            // be handed its command back while something else also answered
            // for it.
            ControlPhase::Reserved(_) => {
                reserved = reserved.saturating_add(1);
                true
            }
            ControlPhase::Accepted(command) => {
                cancellable.push((held.token, command));
                false
            }
            ControlPhase::Applying(_) | ControlPhase::Abandoned(_) => {
                indeterminate = indeterminate.saturating_add(1);
                true
            }
            ControlPhase::Owed(_) | ControlPhase::Settled(_) => true,
        });
        ControlCancellation {
            cancellable,
            indeterminate,
            reserved,
        }
    }
}

/// A reservation being taken into acceptance, still undecided.
///
/// Dropping without committing puts the record back in reserve, so a caller
/// that could not publish its queue entry leaves nothing claiming the work was
/// handed over.
#[cfg(unix)]
#[must_use = "an acceptance that is neither committed nor rolled back is a decision not made"]
pub struct ControlAcceptance<'a> {
    held: Option<(std::sync::MutexGuard<'a, ControlCompletions>, usize)>,
}

#[cfg(unix)]
impl ControlAcceptance<'_> {
    /// Nothing is being handed over, so there is nothing to commit or undo.
    pub fn ungoverned() -> Self {
        Self { held: None }
    }

    /// Keep the acceptance.
    pub fn commit(mut self) {
        self.held = None;
    }
}

#[cfg(unix)]
impl Drop for ControlAcceptance<'_> {
    fn drop(&mut self) {
        let Some((mut inner, position)) = self.held.take() else {
            return;
        };
        // Only this handle put it here, and the registry has been held
        // throughout, so nothing else can have moved it on.
        if let ControlPhase::Accepted(command) = inner.records[position].phase {
            inner.records[position].phase = ControlPhase::Reserved(command);
        }
    }
}

/// What the registry can say about one registration.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlRecordState {
    /// Still held: accepted, applying, or owed an acknowledgement.
    Outstanding,
    /// Issued by this registry and no longer held: it reached an outcome, or
    /// it was settled at a cancellation edge. Either way nothing further is
    /// owed for it, which is what a credit needs to know.
    Retired,
    /// This registry cannot answer for it -- the lock is poisoned, or the
    /// token was never issued here. Not an outcome: reading either as one
    /// would close work on the strength of an absent record.
    Unanswerable,
}

/// One abandoned operation, and the cleanup it named.
///
/// The command is carried for what it identifies, never to be run. Whatever it
/// did before its executor went is exactly what nobody here can describe.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlCleanup {
    pub token: ControlCompletionToken,
    pub command: XAuthorityClientControlCommand,
    /// What the operation reported doing before it was abandoned.
    pub steps: ControlSteps,
}

/// Why recording a cleanup was refused.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlCleanupRefusal {
    /// This registry did not issue the registration.
    Foreign,
    /// No record is held for it.
    NoLongerHeld,
    /// It is not an operation whose executor has gone, so cleanup is not what
    /// it is waiting for.
    NotAbandoned,
    /// Work this operation queued elsewhere can still run, so it is not
    /// waiting on a cleanup yet.
    DependentsOutstanding,
    /// The registry cannot be reached.
    Unavailable,
}

/// What reconciling one client's registrations found.
#[cfg(unix)]
#[derive(Debug, Default)]
pub struct ControlReconciliation {
    /// Accepted and never started. Still held here, still truthfully
    /// unexecuted, and handed on when the instance closes.
    pub unexecuted: usize,
    /// Caught mid-application with their executor gone. Retained, and owed
    /// the cleanup they name.
    pub abandoned: usize,
    /// Caught mid-application with their executor still there. Left exactly
    /// as they are: an outcome may still be established for them, and nothing
    /// here is entitled to decide it will not be.
    pub applying: usize,
    /// Outcomes established and not yet published. Untouched: republishing is
    /// right and reconciling is not publication.
    pub owed: usize,
    /// Still their producer's, and not this edge's business.
    pub reserved: usize,
    /// Whether the registry could be read at all. A reconciliation that found
    /// nothing because nothing could be looked at is not a reconciliation.
    pub readable: bool,
}

#[cfg(unix)]
impl ControlReconciliation {
    fn unavailable() -> Self {
        Self {
            readable: false,
            ..Self::default()
        }
    }
}

/// What a cancellation edge found.
#[cfg(unix)]
#[derive(Debug, Default)]
pub struct ControlCancellation {
    /// Accepted and never started, so truthfully cancellable, each with the
    /// registration it was held under.
    pub cancellable: Vec<(ControlCompletionToken, XAuthorityClientControlCommand)>,
    /// Started with no established outcome. Retained rather than reported as
    /// unexecuted, because the runtime may already have changed.
    pub indeterminate: usize,
    /// Reserved and never accepted. Retained, because the producer that holds
    /// the command is the one that will decide its fate.
    pub reserved: usize,
}
