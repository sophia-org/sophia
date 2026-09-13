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
/// Not a client's transaction id. `send_ack` sees only the client and the
/// public transaction, and those alias across requests that share a
/// transaction, so a mapping built from them would answer one request with
/// another's outcome.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlCompletionToken {
    /// Which registry issued it. Two instances never share a value.
    origin: u64,
    /// Which operation within that registry.
    incarnation: u64,
}

/// How far one control operation has got.
///
/// What is retained differs by phase, and collapsing the three loses the case
/// that matters most.
#[cfg(unix)]
enum ControlPhase {
    /// Accepted and not executed. The command itself is kept, because it can
    /// still be executed or cancelled and there is no outcome yet.
    Accepted(XAuthorityClientControlCommand),
    /// Execution began and no outcome is established. The runtime may already
    /// have mutated, so this is neither unexecuted nor answered; it stays this
    /// way until something establishes what happened. Inventing an outcome to
    /// leave this state would be fabricating a receipt.
    ///
    /// The command is kept for the identity and cleanup responsibility it
    /// carries, not to be replayed: replaying a partly applied command is the
    /// mistake this phase exists to prevent.
    #[allow(dead_code)]
    Applying(XAuthorityClientControlCommand),
    /// An outcome is known and its acknowledgement has not been published.
    /// The effect has happened, so this is republished, never replayed.
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
            Self::Accepted(command) | Self::Applying(command) => command.client,
            Self::Owed(acknowledgement) => acknowledgement.client,
        }
    }
}

/// Why a completion could not be registered.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlCompletionRefusal {
    /// No record storage remains.
    AtCapacity,
    /// The registry cannot be reached.
    Unavailable,
    /// This client's writer has gone, so accepting a command for it would
    /// accept work that nothing is left to execute.
    Sealed,
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

#[cfg(unix)]
struct ControlCompletions {
    records: Vec<(ControlCompletionToken, ControlPhase)>,
    next_incarnation: u64,
    capacity: usize,
    /// Clients whose writer has gone. Per client, not per instance: one
    /// client's writer exiting says nothing about the others, and sealing the
    /// whole registry for it would refuse work every remaining client could
    /// still have executed.
    ///
    /// The records themselves stay: sealing says nothing further will be
    /// answered, not that the outstanding work is resolved.
    sealed: BTreeSet<XServerFrontendClientId>,
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
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            origin: CONTROL_COMPLETION_ORIGINS.fetch_add(1, Ordering::Relaxed),
            inner: Arc::new(Mutex::new(ControlCompletions {
                records: Vec::with_capacity(capacity),
                next_incarnation: 1,
                capacity,
                sealed: BTreeSet::new(),
            })),
        }
    }

    /// Register before the work is accepted.
    ///
    /// Taking the storage first is what makes every later transition
    /// possible: a record that could be refused after acceptance would leave
    /// an accepted command with no owner.
    pub fn register(
        &self,
        command: XAuthorityClientControlCommand,
    ) -> Result<ControlCompletionToken, (ControlCompletionRefusal, XAuthorityClientControlCommand)>
    {
        let Ok(mut inner) = self.inner.lock() else {
            return Err((ControlCompletionRefusal::Unavailable, command));
        };
        if inner.sealed.contains(&command.client) {
            return Err((ControlCompletionRefusal::Sealed, command));
        }
        if inner.records.len() >= inner.capacity {
            return Err((ControlCompletionRefusal::AtCapacity, command));
        }
        let token = ControlCompletionToken {
            origin: self.origin,
            incarnation: inner.next_incarnation,
        };
        inner.next_incarnation = inner.next_incarnation.saturating_add(1);
        inner.records.push((token, ControlPhase::Accepted(command)));
        Ok(token)
    }

    /// Record that execution has begun and no outcome is established yet.
    pub fn begin_applying(&self, token: ControlCompletionToken) {
        let Ok(mut inner) = self.inner.lock() else {
            return;
        };
        for (held, phase) in inner.records.iter_mut() {
            if *held != token {
                continue;
            }
            if let ControlPhase::Accepted(command) = phase {
                *phase = ControlPhase::Applying(*command);
            }
            return;
        }
    }

    /// Record the outcome of trying to publish an acknowledgement.
    ///
    /// A delivered acknowledgement retires the record. A full channel keeps
    /// the exact acknowledgement to publish later. A gone receiver is neither:
    /// nothing was published, so the record stays owed rather than closed on
    /// the strength of a call that returned success.
    pub fn publish(
        &self,
        token: ControlCompletionToken,
        acknowledgement: XAuthorityClientControlAck,
        publication: ControlPublication,
    ) {
        let Ok(mut inner) = self.inner.lock() else {
            return;
        };
        match publication {
            ControlPublication::Delivered => {
                inner.records.retain(|(held, _)| *held != token);
            }
            ControlPublication::Retained | ControlPublication::ReceiverGone => {
                for (held, phase) in inner.records.iter_mut() {
                    if *held == token {
                        *phase = ControlPhase::Owed(acknowledgement);
                        return;
                    }
                }
            }
        }
    }

    /// How many operations still have an unanswered record.
    pub fn outstanding(&self) -> usize {
        self.inner.lock().map(|inner| inner.records.len()).unwrap_or(0)
    }

    /// How many are holding an acknowledgement that could not be published.
    pub fn owed(&self) -> usize {
        self.inner
            .lock()
            .map(|inner| {
                inner
                    .records
                    .iter()
                    .filter(|(_, phase)| matches!(phase, ControlPhase::Owed(_)))
                    .count()
            })
            .unwrap_or(0)
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
        inner.records.retain(|(_, phase)| {
            let ControlPhase::Owed(acknowledgement) = phase else {
                return true;
            };
            match publish(acknowledgement) {
                ControlPublication::Delivered => {
                    delivered = delivered.saturating_add(1);
                    false
                }
                ControlPublication::Retained | ControlPublication::ReceiverGone => true,
            }
        });
        delivered
    }

    /// What became of one registration.
    ///
    /// Absence only means retirement for a token this registry issued. A
    /// foreign or not-yet-issued token is absent too, and treating that as an
    /// outcome would answer for work this registry never accepted.
    pub fn state_of(&self, token: ControlCompletionToken) -> ControlRecordState {
        if token.origin != self.origin {
            return ControlRecordState::Unanswerable;
        }
        let Ok(inner) = self.inner.lock() else {
            return ControlRecordState::Unanswerable;
        };
        if inner.records.iter().any(|(held, _)| *held == token) {
            ControlRecordState::Outstanding
        } else {
            // Origins come from a counter that never repeats and clones share
            // one, so a matching origin means this registry issued the token.
            // Having issued it and no longer holding it leaves only one
            // reading: it was settled.
            ControlRecordState::Retired
        }
    }

    /// Record that the writer answering for one client has gone.
    ///
    /// Returns how many of that client's records were still outstanding. It
    /// does not settle them: the writer that seals is not the owner of the
    /// queue those commands came from, and deciding their fate here would
    /// destroy work with nowhere to put it. Sealing closes the door on new
    /// registrations for that client and leaves settlement to the owner.
    pub fn seal_client(&self, client: XServerFrontendClientId) -> usize {
        let Ok(mut inner) = self.inner.lock() else {
            return 0;
        };
        inner.sealed.insert(client);
        inner
            .records
            .iter()
            .filter(|(_, phase)| phase.client() == client)
            .count()
    }

    /// Whether a writer is still expected to answer for this client.
    pub fn is_sealed(&self, client: XServerFrontendClientId) -> bool {
        self.inner
            .lock()
            .map(|inner| inner.sealed.contains(&client))
            .unwrap_or(true)
    }

    /// Give up this record because the operation now has another owner.
    ///
    /// Only an unexecuted command can be handed on, so only that phase is
    /// discarded here. An applying operation still needs its cleanup
    /// responsibility recorded, and an owed acknowledgement is the only copy
    /// of an outcome that already happened -- discarding either would lose
    /// what the record exists to hold.
    pub fn discard(&self, token: ControlCompletionToken) -> bool {
        let Ok(mut inner) = self.inner.lock() else {
            return false;
        };
        let mut discarded = false;
        inner.records.retain(|(held, phase)| {
            if *held != token || !matches!(phase, ControlPhase::Accepted(_)) {
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
    /// Returns the commands that had not begun applying, which can still be
    /// cancelled truthfully, and separately counts those that had. A command
    /// caught mid-application is not reported as unexecuted: the runtime may
    /// already have changed, and saying otherwise would be inventing the
    /// outcome this whole record exists to avoid inventing.
    pub fn cancel_unfinished(&self) -> ControlCancellation {
        let Ok(mut inner) = self.inner.lock() else {
            return ControlCancellation::default();
        };
        let mut cancellable = Vec::new();
        let mut indeterminate = 0usize;
        inner.records.retain(|(_, phase)| match phase {
            ControlPhase::Accepted(command) => {
                cancellable.push(*command);
                false
            }
            ControlPhase::Applying(_) => {
                indeterminate = indeterminate.saturating_add(1);
                true
            }
            ControlPhase::Owed(_) => true,
        });
        ControlCancellation {
            cancellable,
            indeterminate,
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

/// What a cancellation edge found.
#[cfg(unix)]
#[derive(Debug, Default)]
pub struct ControlCancellation {
    /// Accepted and never started, so truthfully cancellable.
    pub cancellable: Vec<XAuthorityClientControlCommand>,
    /// Started with no established outcome. Retained rather than reported as
    /// unexecuted, because the runtime may already have changed.
    pub indeterminate: usize,
}
