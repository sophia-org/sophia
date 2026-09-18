// Who keeps one connection's join evidence, established before anything can
// produce a result that needs keeping.
//
// WHAT THIS IS AND IS NOT. It makes every operation BORROW an evidence home
// somebody else already owns, instead of handing back the only handle and
// hoping the caller stores it. That is a real property and it is enforced
// here. It is NOT a service lifetime: no production constructor makes one of
// these, so nothing here establishes that the running server has such an owner
// above its shutdown and error paths. Naming a type durable would not
// establish it either.

/// Where one connection's worker lives, and what it leaves behind.
///
/// OWNED BY THE CUSTODY, BESIDE THE HOME ITS RESULT GOES INTO. The three
/// belong together: a thread's handle, the note that thread leaves about how
/// it went, and the place its join result is published. Keeping the first two
/// in whichever frame happened to start the worker left them with a different
/// owner from the third, so a connection's evidence had an external keeper and
/// its worker had none.
///
/// EMPTY AND UNSTARTED WHEN IT IS MADE. Reserving this is storage and nothing
/// else: no thread, no permit, no schedule.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Reached by a caller no production site has yet.
struct PrivateWorkerSource {
    /// This connection's one worker-handle slot.
    ///
    /// ONE PER CONNECTION, AND THE SAME ONE THROUGHOUT. Its lifecycle is the
    /// slot's own -- never started, running, handed on -- and nothing here
    /// resets it because a connection got a new view of it.
    slot: Mutex<PrivateWorkerSlot>,
    /// Where that worker says how it went.
    ///
    /// BEHIND AN `Arc` SO THE BODY CAN BE GIVEN THE SINK AND NOTHING ELSE. A
    /// worker needs somewhere to leave its classification; what it must not be
    /// handed is the custody, the registration or the service owner, any of
    /// which would put the thread on the owning side of the graph it is being
    /// watched by.
    ///
    /// AND IT IS A DIAGNOSTIC, NOT EVIDENCE OF A JOIN. What is written here is
    /// whatever the body wrote. It cannot establish that a thread ended, that
    /// it panicked, or that anything may be started again; only the join
    /// result can.
    exit: Arc<PrivateWorkerExit>,
    /// The capabilities this connection's execution is driven by.
    ///
    /// INERT AND EMPTY UNTIL SOMETHING PREPARES IT, and published once. This
    /// is bounded storage on the source the reservation already made -- not a
    /// second inventory, not a budget, and not a permit.
    control: std::sync::OnceLock<PrivateControlCredentials>,
    /// What this connection's destruction owes, shared with its registration.
    ///
    /// HELD, BECAUSE THE RESPONSIBILITY MUST OUTLIVE THE HANDLE. Holding it
    /// runs nothing; what runs it is still the registration's own `Drop`.
    cleanup: Arc<PrivateCleanupRecord>,
    /// Whether this connection still admits a start, and what its departure
    /// established.
    ///
    /// RESERVED WITH EVERYTHING ELSE, admitting and undecided. A decision that
    /// lived in the frame that made it would be one nothing could recover,
    /// which is the whole reason this is here.
    departure: Mutex<PrivateDepartureState>,
    /// What the service established when it visited this connection to start
    /// its worker, recorded once.
    ///
    /// ONE ATTEMPT PER SOURCE. A later visit finds this and does not respawn,
    /// replace or retry. Bounded storage on the source, not a history.
    attachment: std::sync::OnceLock<PrivateAttachment>,
    /// The gate this connection's queue was minted with.
    ///
    /// THE EXACT ONE, GIVEN TO THIS RESERVATION BEFORE THE ROW WENT IN. Not
    /// one minted here, not one found by client number, and not one handed in
    /// later by whoever happens to be fencing: the sender, the row, the
    /// registration and this source all name the same gate because they were
    /// all given it.
    gate: Arc<PrivateHandoverGate>,
    /// Where this connection's fencing publishes what its gate said.
    ///
    /// RESERVED WITH EVERYTHING ELSE, NotAttempted and empty. The one right to
    /// attempt lives in here, so two views cannot each mint their own.
    fence: PrivateFenceEvidence,
}

/// One connection's evidence custody, owned outside every operation.
///
/// ESTABLISHED FIRST, IN THE SCOPE THAT OUTLIVES WHAT IT PROTECTS. Its own
/// fields keep the store and the publication home; a reaping, a fencing and a
/// commitment borrow them. None of those is the final owner and none of them
/// can empty this by finishing, returning or unwinding.
///
/// WHY THAT ORDER. A join result is published into this home, and a home
/// created inside the operation that fills it is one whose only handle is in
/// that operation's frame. Returning it afterwards offers a keeper; it does
/// not make one, and a caller that dropped it or unwound would take the
/// evidence with it while the obligation stayed outstanding. Owning it first
/// is the difference between offering and having.
///
/// ONE CONNECTION, ONE HOME. This is not a pool, an admission limit, a sharing
/// scheme or a collection of past results, and nothing accumulates in it.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // No production constructor makes one yet.
struct PrivateEvidenceCustody {
    /// The store this connection's place and obligation live in.
    ///
    /// OWNED, because evidence about a connection is no use without the store
    /// that names it, and the whole point of this type is that both outlive
    /// the frames that use them.
    store: PrivateSettlementOwner,
    /// Which connection's evidence this keeps.
    identity: PrivateMaintenanceIdentity,
    /// Where this connection's worker lives and what it leaves behind.
    ///
    /// RESERVED WITH THE HOME, ON THE SAME CREDIT AND BEFORE THE SAME
    /// BOUNDARY. A connection published with a keeper for its result and no
    /// owned place for its handle would be one whose worker belonged to
    /// whichever frame started it.
    source: PrivateWorkerSource,
    /// The publication home a join will write into.
    ///
    /// ALLOCATED HERE, BEFORE ANY HANDLE IS CONSUMED. What is published into
    /// it later goes into a home that already had an owner, so losing the
    /// operation that published it loses the operation and not the result.
    ///
    /// AND IT CARRIES ITS OWN PUBLICATION RIGHT. Sharing the home moved the
    /// question of who may write it out of every operation: the one right sits
    /// in here, an operation takes it or is refused, and it comes back only
    /// from an attempt that consumed nothing.
    join: Arc<PrivateJoinEvidence>,
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // No production constructor makes one yet.
impl PrivateEvidenceCustody {
    /// Take custody of one connection's evidence, before anything produces
    /// any.
    ///
    /// PREPARATION REFUSES NOTHING AND CONSUMES NOTHING. The lease, the worker
    /// handle and whatever evidence already exists stay with their own owners;
    /// this allocates a home and takes handles, which is the order that
    /// matters -- a custody that consumed first and allocated afterwards would
    /// have the same hole it exists to close.
    fn prepared_for(
        store: &PrivateSettlementOwner,
        identity: PrivateMaintenanceIdentity,
        gate: Arc<PrivateHandoverGate>,
        cleanup: Arc<PrivateCleanupRecord>,
    ) -> Self {
        Self {
            store: store.clone(),
            identity,
            source: PrivateWorkerSource {
                // EMPTY AND UNSTARTED. Storage now; a worker only if something
                // later starts one, which nothing here does.
                slot: Mutex::new(PrivateWorkerSlot::empty()),
                exit: Arc::new(PrivateWorkerExit::unstarted()),
                control: std::sync::OnceLock::new(),
                attachment: std::sync::OnceLock::new(),
                cleanup,
                departure: Mutex::new(PrivateDepartureState {
                    admitted: true,
                    published: None,
                    deciding: false,
                    observed: None,
                }),
                gate,
                fence: PrivateFenceEvidence::unattempted(),
            },
            join: Arc::new(PrivateJoinEvidence {
                // THE RIGHT TO PUBLISH STARTS HERE, in the home, unheld. An
                // operation acquires it from the home rather than arriving
                // with one of its own, so a second view of the same home
                // cannot mint the authority to write over what a first
                // attempt established.
                producer: std::sync::atomic::AtomicBool::new(true),
                phase: std::sync::atomic::AtomicU8::new(0),
                result: std::sync::OnceLock::new(),
            }),
        }
    }

    /// The store this custody keeps.
    fn store(&self) -> &PrivateSettlementOwner {
        &self.store
    }

    /// Which connection this custody is for.
    fn identity(&self) -> &PrivateMaintenanceIdentity {
        &self.identity
    }

    /// This connection's worker-handle slot.
    ///
    /// LENT, NOT HANDED OVER. A caller starts into it, hands a handle out of
    /// it or reads its lifecycle; what owns it is this custody, so an
    /// operation that ends -- by returning, refusing or unwinding -- leaves a
    /// started worker's handle exactly where it was.
    fn worker_slot(&self) -> &Mutex<PrivateWorkerSlot> {
        &self.source.slot
    }

    /// The sink this connection's worker writes its classification into.
    ///
    /// CLONEABLE ON PURPOSE, AND ONLY THIS. A body that needs to leave a note
    /// takes a handle to the note, not to the connection.
    fn exit_sink(&self) -> &Arc<PrivateWorkerExit> {
        &self.source.exit
    }

    /// This connection's gate.
    ///
    /// LENT. A caller asks it to close or reads what it said; what owns it is
    /// this custody and the row it was published with.
    fn gate(&self) -> &Arc<PrivateHandoverGate> {
        &self.source.gate
    }

    /// Where this connection's fencing publishes its answer.
    fn fence_evidence(&self) -> &PrivateFenceEvidence {
        &self.source.fence
    }

    /// What this connection's destruction is responsible for.
    ///
    /// THE REGISTRATION'S OWN RECORD, not a copy of it: the handle and this
    /// keeper reach one piece of state, so what is attached or transferred
    /// through one is what the other acts on.
    fn cleanup_record(&self) -> &Arc<PrivateCleanupRecord> {
        &self.source.cleanup
    }

    /// The publication home this custody owns.
    ///
    /// BORROWED, NOT HANDED OVER. A caller reads what is in it; what keeps it
    /// alive is this.
    fn join(&self) -> &Arc<PrivateJoinEvidence> {
        &self.join
    }

    /// A view of this custody for the length of one operation.
    fn view(&self) -> PrivateCustodyView<'_> {
        PrivateCustodyView(self)
    }
}

/// One operation's view of a connection's custody.
///
/// A BORROW, AND ENDING IT ENDS THE BORROW. There is no `Drop` here and there
/// must not be one that does anything: a scope going away is not a keeper
/// letting go, not a place being returned, not an identity being retired and
/// not a join or a fence becoming established. Losing a view loses a view.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Borrowed by callers no production site has yet.
struct PrivateCustodyView<'a>(&'a PrivateEvidenceCustody);

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Borrowed by callers no production site has yet.
impl PrivateCustodyView<'_> {
    fn store(&self) -> &PrivateSettlementOwner {
        self.0.store()
    }

    fn identity(&self) -> &PrivateMaintenanceIdentity {
        self.0.identity()
    }

    fn join(&self) -> &Arc<PrivateJoinEvidence> {
        self.0.join()
    }
}
