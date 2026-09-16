/// What asking for a serving owner did.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Nothing starts one yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PrivateOrderedPromotion {
    /// An owner exists and nothing drives it.
    Ready,
    /// There is no bound transport here to promote.
    Unbound,
    /// One is already here, and it stands.
    AlreadyServing,
    /// This endpoint is closed to handovers, or its wire is ended: nothing
    /// starts on it.
    Closing,
    /// The endpoint's gate could not be read, so whether it is closed was not
    /// established. Distinct from a closure: one is a decision, the other is
    /// the absence of one.
    EndpointUnreadable,
    /// Preparation refused. Everything is where it was.
    Refused(X11OrderedServingRefusal),
    /// The storage could not be read, so nothing was attempted.
    Unreadable,
}

// One connection's ordered output, from the receiver a registration mints to
// the binding that pairs it with that connection's socket, and the closing
// that establishes nothing more will be handed over.
//
// Split by subject from the registry that publishes the row. That file answers
// what a client's routes are and who may reach them; this answers what the
// ordered half of one connection IS, how it comes to be paired with the socket
// it belongs to, and what ending it establishes.

/// One connection's ordered receiver, minted with the registration that owns
/// it.
///
/// MINTED HERE AND NOWHERE ELSE, in the same expression that makes the channel
/// and beside the registration that gets the other end. A serving owner that
/// accepted a bare receiver could be handed one connection's registration and
/// another's queue, and nothing about either value would say so; a receiver
/// that carries the registration cell it was made with can be asked.
///
/// There is deliberately no constructor taking a receiver and a witness: one
/// would let a caller assert exactly the association this exists to establish.
#[cfg(unix)]
struct XAuthorityOrderedReceiver {
    receiver: Receiver<XAuthorityOrderedDelivery>,
    /// The connection-state cell this registration is, by pointer.
    registration: Arc<std::sync::OnceLock<PrivateAppliedClientState>>,
    /// How many this queue can hold at once.
    ///
    /// Carried because a holder of the receiver cannot ask a channel its
    /// capacity, and anything that must reserve room for what this queue can
    /// deliver has to know the number rather than pick one.
    capacity: usize,
    /// The notice this connection's senders publish to.
    ///
    /// Held by the receiving half so whoever owns this queue has something to
    /// wait on. Nothing here waits yet: the worker that would is not landed.
    ///
    /// IT DOES NOT OUTLIVE THIS WRAPPER BY ITSELF. Taking the receiver out
    /// leaves the wrapper behind and this handle with it, so whoever takes the
    /// receiver must carry the notice across too -- which the serving owner's
    /// construction does, cloning it before the take. The senders hold the
    /// other end, so the notice itself lives as long as any of them does.
    #[cfg_attr(not(test), allow(dead_code))] // Nothing waits on it yet.
    wake: Arc<PrivateOrderedWake>,
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // The per-connection loop is not attached yet.
impl XAuthorityOrderedReceiver {
    /// Whether this receiver was minted by exactly this registration.
    fn minted_by(&self, registration: &XServerFrontendClientRouteRegistration) -> bool {
        Arc::ptr_eq(&self.registration, &registration.connection_state)
    }

    /// Give up the receiver itself, once its provenance has been established.
    fn into_receiver(self) -> Receiver<XAuthorityOrderedDelivery> {
        self.receiver
    }

    fn capacity(&self) -> usize {
        self.capacity
    }
}


/// Reading a connection's queued output does not need its provenance, so the
/// ordinary receiver operations are available directly. Taking ownership of
/// the receiver does need it, and that goes through `into_receiver`.
#[cfg(unix)]
impl std::ops::Deref for XAuthorityOrderedReceiver {
    type Target = Receiver<XAuthorityOrderedDelivery>;

    fn deref(&self) -> &Self::Target {
        &self.receiver
    }
}

#[cfg(unix)]
impl XServerFrontendClientRouteRegistration {
    /// Bind this connection's ordered queue to this connection's own output,
    /// and take custody of whichever half survives.
    ///
    /// THE DECISION LIVES HERE, not at the call site. The call site owns the
    /// accepted stream and this registration, which is what makes the pairing
    /// sound, but what to do with a refused binding is a rule about accepted
    /// work and belongs where the rest of those rules are -- and where a
    /// control can reach it.
    ///
    /// A REFUSED BINDING STILL HAS A QUEUE. The receiver was published with
    /// this connection's row, so it may already hold capsules; it is retained
    /// with the refusal that stopped it rather than dropped. What it cannot
    /// have is a handle on the connection, so nothing will be able to end that
    /// wire later -- which is the honest cost of the refusal and is recorded,
    /// not smoothed over.
    ///
    /// `Err` means this registration already holds custody: nothing is taken,
    /// and the receiver goes back to the caller rather than being replaced
    /// over work that may already be on it.
    #[allow(clippy::result_large_err)] // The receiver travels back rather than being dropped.
    pub(crate) fn bind_ordered_output(
        &self,
        ordered: XAuthorityOrderedReceiver,
        output: &Arc<Mutex<UnixStream>>,
        wire: &Arc<X11WirePermission>,
        control_pending: &Arc<AtomicUsize>,
    ) -> Result<Option<X11OrderedServingRefusal>, XAuthorityOrderedReceiver> {
        // THE CAUSE IS RECORDED WITH THE CUSTODY, not reconstructed later. A
        // teardown that wrote one reason for every connection would say
        // "never served" over a binding that actually failed for want of a
        // second handle on the socket, and whoever inherits the queue would be
        // looking for a worker that was never the problem.
        let (accepted, refused) = match XAuthorityOrderedTransport::bind(
            self,
            ordered,
            output,
            wire,
            control_pending,
            None,
        ) {
            // Bound, and no owner has been built on it -- which is what a
            // connection's ordered output is until a worker exists.
            Ok(transport) => (
                PrivateOrderedSetupCustody::Transport(Box::new(transport)),
                X11OrderedServingRefusal::Unserved,
            ),
            Err((refusal, ordered)) => (
                PrivateOrderedSetupCustody::Receiver(Box::new(ordered)),
                refusal,
            ),
        };
        let reported = (refused != X11OrderedServingRefusal::Unserved).then_some(refused);
        match self.retain_ordered_setup(PrivateOrderedContinuation::Setup {
            accepted,
            refusal: refused,
            // Nothing is established yet: the connection is being built, not
            // torn down, and no worker has been started for it. Teardown
            // writes what its close found and what became of any worker.
            evidence: PrivateOrderedEvidence::unstarted(),
            // Nothing has been received off this queue. Receiving is how a
            // driver learns whether producers are gone, and nothing here is
            // driving.
            retained: Vec::new(),
            drained: false,
            ended: false,
            ending_refused: None,
        }) {
            Ok(()) => Ok(reported),
            // Already bound. Whatever was offered here comes back out; the
            // first custody stays, because it may hold accepted capsules.
            Err(PrivateOrderedContinuation::Setup {
                accepted: PrivateOrderedSetupCustody::Receiver(ordered),
                ..
            }) => Err(*ordered),
            Err(PrivateOrderedContinuation::Setup {
                accepted: PrivateOrderedSetupCustody::Transport(transport),
                ..
            }) => Err(transport.ordered),
            Err(PrivateOrderedContinuation::Serving { .. }) => {
                unreachable!("this function builds a setup continuation")
            }
        }
    }

    /// Turn this connection's bound transport into a serving owner, in place.
    ///
    /// READY, AND DRIVEN BY NOBODY. What this makes is an owner that exists;
    /// it does not start a worker, does not serve, does not receive, and does
    /// not answer anything. A connection is in exactly one of two payload
    /// states afterwards and the variant says which: a bound transport with no
    /// owner, or an owner that has never been started.
    ///
    /// THE TRANSPORT IS BORROWED FIRST. Preparation reads what it needs from
    /// it and does every fallible and allocating thing -- provenance, the
    /// endpoint, the retention its queue implies, the home the owner will live
    /// in -- while the transport is still in this registration's storage. Only
    /// then is it taken out, and what runs between that take and the
    /// assignment cannot fail and does not allocate.
    ///
    /// A REFUSAL CONSUMES NOTHING. The transport stays exactly where it was,
    /// with whatever its queue is holding, and the connection is as it was.
    ///
    /// THE FIRST OWNER STANDS. A second promotion does not rebuild it, does
    /// not reset its identity, close state, attempt budget, frame progress or
    /// held admissions, and does not start anything: it is refused, and says
    /// that an owner is already there.
    #[cfg_attr(not(test), allow(dead_code))] // Nothing promotes in production yet.
    pub(crate) fn promote_ordered_serving(
        &self,
        frontend: &crate::x11_socket::PrivateXServerFrontend,
    ) -> PrivateOrderedPromotion {
        // THE LIVE ENDPOINT, NOT WHAT TEARDOWN ONCE SAW. The evidence on a
        // payload records what a close established at teardown; it says
        // nothing about whether this endpoint is open now, and a close made
        // through the real interface while the registration lives leaves it
        // untouched. Reading eligibility from it admitted an owner onto an
        // endpoint that was already closed.
        //
        // HELD ACROSS THE CHECK AND THE COMMIT. The gate is what puts a close
        // and everything that must not straddle one into a single order, so
        // taking it here means a concurrent close either happens before this
        // -- and refuses it -- or after, with the owner already made. There is
        // no window between asking and acting.
        let entered = match self.ordered_gate.entered() {
            Ok(entered) => entered,
            Err(PrivateHandoverRefusal::Fenced) => return PrivateOrderedPromotion::Closing,
            Err(PrivateHandoverRefusal::Unreadable) => {
                return PrivateOrderedPromotion::EndpointUnreadable;
            }
        };
        let Some(promotion) = self.ordered_home.occupy(|held| {
        match held.as_ref() {
            Some(PrivateOrderedContinuation::Serving { .. }) => {
                return PrivateOrderedPromotion::AlreadyServing;
            }
            Some(PrivateOrderedContinuation::Setup {
                accepted: PrivateOrderedSetupCustody::Transport(_),
                ended,
                ending_refused,
                evidence,
                ..
            }) if !*ended && ending_refused.is_none() => {
                // The payload's own history still matters: a wire that has
                // been ended, or refused an ending, is not one to start on
                // however open the endpoint is.
                let _ = evidence;
            }
            // A connection whose endpoint has been closed, or whose wire has
            // been ended or refused an ending, is not one anything may be
            // started on.
            Some(PrivateOrderedContinuation::Setup {
                accepted: PrivateOrderedSetupCustody::Transport(_),
                ..
            }) => return PrivateOrderedPromotion::Closing,
            // A receiver alone has no connection to serve, and nothing at all
            // has nothing to promote.
            Some(PrivateOrderedContinuation::Setup { .. }) | None => {
                return PrivateOrderedPromotion::Unbound;
            }
        }
        let prepared = {
            let Some(PrivateOrderedContinuation::Setup {
                accepted: PrivateOrderedSetupCustody::Transport(transport),
                ..
            }) = held.as_ref()
            else {
                unreachable!("checked above")
            };
            match X11OrderedServingOwner::prepare_for_registration(frontend, self, transport) {
                Ok(prepared) => prepared,
                Err(refusal) => return PrivateOrderedPromotion::Refused(refusal),
            }
        };
        // FROM HERE TO THE ASSIGNMENT: no allocation, no call that can fail,
        // no lock, no callback. The home was made during preparation and the
        // evidence is carried across unchanged.
        let Some(PrivateOrderedContinuation::Setup {
            accepted: PrivateOrderedSetupCustody::Transport(transport),
            evidence,
            ..
        }) = held.take()
        else {
            unreachable!("checked above")
        };
        *held = Some(PrivateOrderedContinuation::Serving {
            owner: prepared.commit(*transport),
            evidence,
        });
        PrivateOrderedPromotion::Ready
        }) else {
            return PrivateOrderedPromotion::Unreadable;
        };
        drop(entered);
        promotion
    }

    /// Take custody of this connection's ordered output.
    ///
    /// WRITTEN DOWN BEFORE IT IS USED. From here the registration owns it, and
    /// every way out of connection setup -- including the ones that refuse
    /// three lines later -- ends with it retained rather than dropped.
    ///
    /// Refuses a second custody rather than replacing one: the first may
    /// already hold accepted capsules, and overwriting it would discard them
    /// with nothing recording that they existed. The rejected custody comes
    /// back to the caller.
    fn retain_ordered_setup(
        &self,
        custody: PrivateOrderedContinuation,
    ) -> Result<(), PrivateOrderedContinuation> {
        // INTO THE HOME THE PLACE ALREADY HOLDS. What binds here is reachable
        // from the place from this moment, not from teardown onwards, which is
        // what lets anything else borrow this connection's output later
        // without owning this registration.
        match self.ordered_home.bind(custody) {
            PrivateHomeBinding::Bound => Ok(()),
            PrivateHomeBinding::Occupied(custody) | PrivateHomeBinding::Ended(custody) => {
                Err(custody)
            }
        }
    }

    /// Close this endpoint to further handovers, irreversibly.
    ///
    /// EXACT BY CONSTRUCTION. The gate is this registration's own, minted with
    /// its queue, so a replacement registration for the same client is
    /// untouched by this -- there is no lookup here that could reach one.
    ///
    /// A handover already inside the gate completes first and this waits for
    /// it. What it establishes is that no FURTHER handover will be admitted;
    /// it does not end a socket, answer a finalizer or settle anything, and
    /// those remain separate facts to be established separately.
    pub(crate) fn fence_ordered_handovers(&self) -> PrivateHandoverFence {
        self.ordered_gate.close()
    }

    /// This connection's gate, for a holder that will close it later.
    ///
    /// THE CAPABILITY, NOT THE REGISTRATION. Something that fences after this
    /// connection's worker has been joined needs the gate and nothing else;
    /// keeping the registration alive to reach one would hold a whole row open
    /// for the sake of a handle it has already published.
    #[cfg_attr(not(test), allow(dead_code))] // Held by a fencing not attached yet.
    pub(crate) fn handover_gate(&self) -> Arc<PrivateHandoverGate> {
        self.ordered_gate.clone()
    }

    /// The name of this connection's obligation, if it has a place.
    ///
    /// FROM THE RESERVATION THAT MADE IT, so a caller gets the name this
    /// connection was actually given rather than one assembled out of an
    /// index, a home and a store it happened to be holding.
    ///
    /// `None` MEANS THIS REGISTRATION IS NOT HOLDING A LEASE, which is not the
    /// same as there being no store or no place. A registry with no
    /// continuation store never had one; a connection whose lease has been
    /// taken for a conversion, or consumed by its own teardown, has not got
    /// one here any more. In neither case does this establish anything about
    /// what the store holds.
    #[cfg_attr(not(test), allow(dead_code))] // Asked by a commitment not attached yet.
    pub(crate) fn maintenance_identity(&self) -> Option<PrivateMaintenanceIdentity> {
        let held = match self.ordered_continuation.lock() {
            Ok(held) => held,
            Err(poisoned) => poisoned.into_inner(),
        };
        held.as_ref()
            .map(PrivateOrderedContinuationSlot::maintenance_identity)
    }

    /// Pin the evidence custody reserved for this connection.
    ///
    /// THE ONE RESERVED BEFORE THIS ROW WAS PUBLISHED, every time. This is not
    /// a way to make a publication home: there is nothing here that allocates,
    /// and a connection registered without a service owner has none and is
    /// told so rather than given a fresh one.
    ///
    /// `None` MEANS NO CUSTODY WAS RESERVED FOR THIS REGISTRATION, which is a
    /// fact about how this registry was built and not about the connection.
    #[cfg_attr(not(test), allow(dead_code))] // Asked by a caller not attached yet.
    pub(crate) fn registered_custody(&self) -> Option<PrivateCustodyReach> {
        self.ordered_custody
            .as_ref()
            .map(PrivateRegisteredCustody::pin)
    }

    /// Whether this endpoint is closed to handovers. `None` if unreadable.
    #[cfg_attr(not(test), allow(dead_code))] // Only controls ask this today.
    pub(crate) fn ordered_handovers_fenced(&self) -> Option<bool> {
        self.ordered_gate.fenced()
    }
}
