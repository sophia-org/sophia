// What one connection's destruction is responsible for, kept where its
// connection's other evidence is.
//
// Split by subject from the registration: a registration is a handle a caller
// holds and drops, and this is the responsibility that handle carries -- its
// reservation accounting, its lifecycle lease, the notifications it owes and
// the routing state that must stop naming it.
//
// WHY IT IS SHARED. Deferring that destruction safely means keeping the
// responsibility somewhere that outlives the handle. This slice moves it; it
// does not change when it runs. Registration Drop remains the only production
// trigger, at the same point and in the same order as before.
//
// KEEPING IT IS NOT RUNNING IT. An inert record that is never published
// executes nothing, and a record retained after its registration has gone does
// not repeat what that registration already did.

/// One connection's teardown responsibility.
///
/// ITS CAPABILITIES ARE THE CONNECTION'S OWN, captured with its reservation
/// and not reconstructed later from a client number. A record assembled from a
/// number would be about whoever holds that number when it is asked.
///
/// IT DOES NOT OWN A REGISTRATION. Nesting the handle whose `Drop` this is
/// inside the thing that outlives it would make keeping the responsibility
/// run it.
#[cfg(unix)]
struct PrivateCleanupRecord {
    lifecycle: Mutex<Option<PrivateConnectionLifecycle>>,
    /// The place this connection's home sits in, reserved before this
    /// connection was exposed.
    ///
    /// Taken by this connection's teardown, which accounts for the place once
    /// the home has said whether anything is owed through it. A lease dropped
    /// without being disposed of is counted as abandoned rather than handed
    /// out again, so a connection that ended with nobody accounting for it is
    /// visible instead of silent.
    #[allow(dead_code)]
    ordered_continuation: Mutex<Option<PrivateOrderedContinuationSlot>>,
    /// Where this connection's ordered output lives, from binding onwards.
    ///
    /// A HANDLE, NOT A STORAGE OF ITS OWN. When there is a place, this is the
    /// very home that place holds: the reservation makes it, and the
    /// registration is handed the same one. So the output is reachable from
    /// the place from the moment it binds rather than from teardown onwards,
    /// and teardown has nothing to move -- which is what lets anything else
    /// borrow this connection's output without owning this registration.
    ///
    /// A REGISTRY WITH NO CONTINUATION STORE STILL HAS ONE. There is no place
    /// for it to sit in, so it is this registration's alone and goes when the
    /// registration does, which is what a connection with nowhere to hand over
    /// to has always done.
    ordered_home: Arc<PrivateOrderedHome>,
    /// Where this registration's handovers are serialized with its closing.
    ///
    /// Held here as well as in the row, because closing is this
    /// registration's act: a close that had to find the gate by client id
    /// could reach a replacement's.
    ordered_gate: Arc<PrivateHandoverGate>,
    connection_state: Arc<std::sync::OnceLock<PrivateAppliedClientState>>,
    input_recovery: InputRecovery,
    client: XServerFrontendClientId,
    /// The completion registry this client's control is answered through,
    /// when the instance is private. Held so that losing the registration is
    /// an edge this client's control records are told about, rather than one
    /// that quietly leaves them waiting for a writer that has gone.
    control_completion: Arc<std::sync::OnceLock<ControlCompletionRegistry>>,
    clients: Arc<Mutex<BTreeMap<XServerFrontendClientId, XServerFrontendClientRouteSenders>>>,
    surfaces: Arc<Mutex<BTreeMap<SurfaceId, XServerFrontendSurfaceRoute>>>,
    focused_surface: Arc<Mutex<Option<XServerFrontendSurfaceRoute>>>,
    window_parents:
        Arc<Mutex<BTreeMap<(XServerFrontendClientId, XResourceId), XResourceId>>>,
    core_event_subscriptions:
        Arc<Mutex<BTreeMap<(XServerFrontendClientId, XResourceId), u32>>>,
    randr_subscriptions: Arc<Mutex<BTreeMap<XServerFrontendClientId, (XResourceId, u16)>>>,
    /// Selections a client watches, keyed by the window it named when it
    /// subscribed. One client may watch several selections, and the same
    /// selection through different windows, so the window is part of the key
    /// rather than a value that the next subscription overwrites.
    xfixes_selection_subscriptions: XFixesSelectionSubscriptions,
    present_subscriptions:
        Arc<Mutex<BTreeMap<(XServerFrontendClientId, XResourceId), XPresentSubscription>>>,
    pending_presentations: Arc<XPendingPresentRegistry>,
    frozen_input: Arc<Mutex<VecDeque<XDeferredRoutedInput>>>,
}

#[cfg(unix)]
impl PrivateCleanupRecord {
    /// One connection's responsibility, prepared before its row is published.
    ///
    /// EVERY CAPABILITY IS THIS REGISTRY'S OWN AND THIS CONNECTION'S OWN. The
    /// place, the home and the gate come from the reservation that made them;
    /// the tables come from the registry that will have to stop naming this
    /// client. Nothing here is looked up afterwards.
    #[allow(clippy::too_many_arguments)]
    fn prepared_for(
        registry: &XServerFrontendRouteRegistry,
        client: XServerFrontendClientId,
        continuation: Option<PrivateOrderedContinuationSlot>,
        home: Arc<PrivateOrderedHome>,
        gate: Arc<PrivateHandoverGate>,
        connection_state: Arc<std::sync::OnceLock<PrivateAppliedClientState>>,
    ) -> Self {
        Self {
            lifecycle: Mutex::new(None),
            ordered_continuation: Mutex::new(continuation),
            ordered_home: home,
            ordered_gate: gate,
            connection_state,
            input_recovery: registry.input_recovery.clone(),
            client,
            control_completion: registry.control_completion.clone(),
            clients: registry.clients.clone(),
            surfaces: registry.surfaces.clone(),
            focused_surface: registry.focused_surface.clone(),
            window_parents: registry.window_parents.clone(),
            core_event_subscriptions: registry.core_event_subscriptions.clone(),
            randr_subscriptions: registry.randr_subscriptions.clone(),
            xfixes_selection_subscriptions: registry.xfixes_selection_subscriptions.clone(),
            present_subscriptions: registry.present_subscriptions.clone(),
            pending_presentations: registry.pending_presentations.clone(),
            frozen_input: registry.frozen_input.clone(),
        }
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

    /// Whether this connection holds a place at all.
    fn holds_a_place(&self) -> bool {
        self.maintenance_identity().is_some()
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

    /// Close this endpoint and move what it still owes into its own place.
    ///
    /// THE FENCE IS WHAT MAKES THE MOVE SOUND. Once it is established no
    /// further capsule can be accepted for this connection, so the queue taken
    /// here is the whole of what was accepted -- not a snapshot with a
    /// producer still writing behind it.
    ///
    /// WHAT IS RETAINED IS NOT UNPACKED, AND IT IS NOT CARRIED ANYWHERE. This
    /// connection's output has lived in its home since it bound; what happens
    /// here is that the endpoint is closed, what the closure established is
    /// written beside what that home already holds, and the standing change is
    /// accounted for.
    ///
    /// AND NOTHING IS RECEIVED OFF THE QUEUE. Receiving is how a driver learns
    /// whether producers are gone, and doing it during teardown would mean
    /// deciding the disposition of accepted work on the path least able to
    /// answer for it. Nothing here authorises such a driver.
    ///
    /// AN ENDING CAPABILITY IS THE TRANSPORT'S, OR THERE IS NONE.
    /// A transport carries an independent handle on this connection, so a
    /// retained transport keeps the wire reachable after the connection's own
    /// frame is gone and whoever drives it can still end it.
    ///
    /// A receiver alone carries no such handle. What the retained state then
    /// knows is precisely that: no way to end the wire and no established fact
    /// about it. It does NOT know that the socket closed -- whether it did
    /// depends on who else holds a descriptor for it, which is not this
    /// registration's to say -- and it must not record an ending it cannot
    /// establish. The queue is retained with no way to deliver what is in it,
    /// which is the worse outcome, recorded as what it is rather than smoothed
    /// over; the refusal says why there is no transport.
    ///
    /// Ending the wire is NOT done here. A close is its own act with its own
    /// outcome, and a teardown that ended a wire in passing would report
    /// nothing about whether it worked.
    fn retain_ordered_continuation(&self) {
        // KEPT, AND CARRIED INTO THE RECORD. An Established or
        // AlreadyEstablished fence is what makes moving this queue sound:
        // nothing further will be admitted for it.
        //
        // An Unreadable one is not, and the reason is narrower than it looks.
        // The gate was acquired -- a poisoned lock is an acquired lock, handed
        // back inside the error -- so no producer was inside while this ran
        // and exclusion is not what failed. What is unestablished is that the
        // closure was made over resolved custody: someone panicked in there,
        // and the handover they were making may be half-answered. A record
        // moved under that must not read as closed, so the outcome goes with
        // it rather than being discarded here.
        let fence = self.fence_ordered_handovers();
        // NOTHING IS MOVED HERE, AND THERE IS NOTHING TO MOVE. This
        // connection's output has lived in its home since it bound, and the
        // home has been in the place since the place was reserved. What is
        // left to do is write what this teardown knows and say the connection
        // has ended.
        //
        // WHAT THE MOVE COST WHILE IT EXISTED was not only the risk of losing
        // work on the way: it was that until teardown ran, the only thing able
        // to reach this connection's output was this registration. Anything
        // meant to borrow it later would have had to be handed the payload
        // rather than a way to reach it, and the hand-over would have
        // invalidated whatever it was holding.
        //
        // WRITTEN WHERE IT LIVES. Both shapes carry evidence and both consult
        // it: a record that reached retention without it would carry None for
        // THIS closure, and this is the one its own teardown made. The gate
        // itself survives -- its custody holds it -- but that does not make
        // this answer reconstructable afterwards, because a later close is a
        // different act with an answer of its own. Writing it for one shape
        // and not the other would make a connection's fate depend on how far
        // its setup got.
        // Asked before the borrow below, which goes ahead through the
        // poisoned guard the way everything that must go ahead does.
        let source_poisoned = self.ordered_home.unreadable();
        let wrote = self.ordered_home.borrow(|continuation| {
            let recorded = match continuation {
                PrivateOrderedContinuation::Setup { evidence, .. }
                | PrivateOrderedContinuation::Serving { evidence, .. } => evidence,
            };
            recorded.fence = Some(fence);
            // A home that could not be read is a fact about this connection,
            // not something to launder by writing elsewhere. The home borrows
            // through the poisoned guard, as everything that must go ahead
            // regardless does, and records that it did.
            recorded.source_poisoned = source_poisoned;
            // Nothing started a worker for this connection, so there is
            // nothing to join and no join is manufactured. When a spawn
            // exists, what it left is written here by whoever joined it.
            recorded.worker = PrivateOrderedWorkerExit::NeverStarted;
        });
        let _ = wrote;
        // THE CONNECTION HAS ENDED, and that is said whether or not this
        // registration still holds the place. A conversion may have taken the
        // lease already and left the store's own holder responsible for it;
        // the home is the same home either way, and a holder that never
        // learned its connection had gone would be waiting for a producer that
        // is not coming.
        //
        // AND IT IS SAID BEFORE THE PLACE IS ACCOUNTED FOR, so nothing can
        // find the place disposed of over a home that still reads as live.
        let owed = self.ordered_home.retain();

        // The place was taken before this connection was exposed. Taking the
        // lease out is what says this registration is done with it.
        let slot = match self.ordered_continuation.lock() {
            Ok(mut held) => held.take(),
            Err(poisoned) => poisoned.into_inner().take(),
        };
        let Some(slot) = slot else {
            // No place of this registration's to account for. Either this
            // registry has no continuation store, or the lease has already
            // gone to whoever is responsible now. Nothing here may invent one.
            //
            // NO CUSTODY IS NOT NO WORK, either: this connection's row was
            // published, so capsules may have been accepted into a queue whose
            // receiver went somewhere this registration cannot see. What is
            // owed is recorded in the home above, for whoever holds the place.
            return;
        };
        let _ = slot.commit(owed);
    }

    /// Everything one connection's destruction owes, run once, synchronously.
    ///
    /// UNCHANGED IN CONTENT AND ORDER. This is the body that was the
    /// registration's `Drop`, moved to where the responsibility now lives. The
    /// fence is still taken before the queue is moved, and the writers are
    /// still told before the query state is removed.
    ///
    /// IT IS NOT A PROMISE THAT EVERYTHING FINISHED. The ignored errors below
    /// are the same ignored errors: a lock that could not be taken leaves work
    /// nobody did, and nothing here records that it was done. There is no
    /// phase called Complete, because a function returning is not a settlement.
    ///
    /// AND IT IS NOT YET SAFE TO RUN LATER -- A DEPENDENCY, RECORDED HERE
    /// BECAUSE THE NEW TYPE DOES NOT SOLVE IT. Most of the removals below are
    /// BY CLIENT NUMBER: rows, surfaces, focus, parents, subscriptions,
    /// pending presentations and frozen input all say "whatever is under this
    /// id". Capturing an exact identity when the record was made does not make
    /// those safe to execute afterwards, because a number is reissued and the
    /// connection holding it then is somebody else.
    ///
    /// WHAT A LATER EXECUTION BOUNDARY MUST ESTABLISH is exact-occupant
    /// exclusion for these effects, covering the interval between deciding
    /// and acting. Checking a maintenance identity and then calling
    /// `remove(client)` would not do it: the check and the removal are two
    /// moments, and the reissue can happen between them. Nothing in this slice
    /// defers anything, and this is written down so the next one cannot
    /// mistake the record for the repair.
    fn run_synchronous_cleanup(&self) {
        // A CONNECTION THAT ENDS IS CLOSED TO HANDOVERS, and its queue goes to
        // the place reserved for it.
        //
        // Removing the row below is not what closes it and never was: the
        // capture happens under the client table and the send after it is
        // released, so a row removed here says nothing about a sender already
        // in someone's hand. The gate is what a captured sender is refused by.
        //
        // The fence is taken before the queue is moved. No outcome separates
        // the two today -- nothing is received here, so a capsule accepted in
        // between lands on the same retained queue either way -- and the order
        // is written this way for when a driver receives from that queue,
        // where it will separate.
        self.retain_ordered_continuation();
        // TAKEN, CLOSED AND DROPPED HERE, WHETHER OR NOT THE CELL IS POISONED.
        //
        // WHAT THIS LEASE USED TO GET FOR FREE. It was a field of the handle,
        // so the handle's own destruction dropped it AFTER this body ran, and
        // its destructor closes the gate. The explicit close was the belt; the
        // destructor was the braces. A poisoned cell skipped the close and the
        // destructor still ran.
        //
        // THAT STOPPED BEING TRUE WHEN ITS HOME OUTLIVED THE HANDLE. A lease
        // left in this record on the poison path is not dropped when the
        // registration goes -- the keeper still holds the record -- so the
        // gate stays open for as long as that record lives. Skipping the take
        // here is not "the same as before"; it is the loss of the disposal
        // that used to happen anyway.
        //
        // SO THE GUARD IS RECOVERED, AND ONLY TO TAKE. Taking a lease out in
        // order to dispose of it is a close this connection's destruction is
        // asking for, not a reading of state somebody's panic left behind.
        // Nothing else is read from the cell, nothing is put back, and this
        // recovers no unreadable payload into serving and performs no native
        // cleanup or settlement.
        let lease = match self.lifecycle.lock() {
            Ok(mut held) => held.take(),
            Err(poisoned) => poisoned.into_inner().take(),
        };
        if let Some(lease) = lease {
            // The guard is released above, before the close: what closing
            // takes is the gate's own lock, and holding this cell across it
            // would put a second lock under the first for no reason.
            lease.close();
            drop(lease);
        }
        // Before the route senders go. What was mid-application when the
        // client's registration ended is not unexecuted and is not answered;
        // it is owed the cleanup it named, and saying so here is what keeps
        // that responsibility from ending with the registration.
        if let Some(completion) = self.control_completion.get() {
            // A writer expected for this client is not coming now. If a writer
            // did start, this changes nothing and its own exit is the edge;
            // if startup failed before one ever existed, this is what stops an
            // expectation nothing will meet from keeping the client executing
            // forever. Either way the registry decides what that leaves,
            // under the lock that abandons.
            completion.cancel_expected_writer(self.client);
        }
        let _ = self.input_recovery.disconnect(self.client, XAuthorityInputDeliveryOutcome::ClientDisconnected);
        if let Ok(mut clients) = self.clients.lock() {
            clients.remove(&self.client);
        }
        if let Ok(mut surfaces) = self.surfaces.lock() {
            surfaces.retain(|_, route| route.client != self.client);
        }
        if let Ok(mut focused) = self.focused_surface.lock()
            && focused.is_some_and(|route| route.client == self.client)
        {
            *focused = None;
        }
        if let Ok(mut parents) = self.window_parents.lock() {
            parents.retain(|(client, _), _| *client != self.client);
        }
        if let Ok(mut subscriptions) = self.core_event_subscriptions.lock() {
            subscriptions.retain(|(client, _), _| *client != self.client);
        }
        if let Ok(mut subscriptions) = self.randr_subscriptions.lock() {
            subscriptions.remove(&self.client);
        }
        // Retired here as well as on an orderly close, because a client whose
        // connection failed before that point never reaches it. A client id
        // may be reissued, and an inherited subscription would deliver one
        // client's selections to whoever takes the id next.
        if let Ok(mut subscriptions) = self.xfixes_selection_subscriptions.lock() {
            subscriptions.retain(|(client, _, _), _| *client != self.client);
        }
        if let Ok(mut subscriptions) = self.present_subscriptions.lock() {
            subscriptions.retain(|(client, _), _| *client != self.client);
        }
        if let Ok(mut pending) = self.pending_presentations.entries.lock() {
            pending.retain(|_, presentation| presentation.client != self.client);
            self.pending_presentations.capacity_changed.notify_all();
        }
        let abandoned = if let Ok(mut frozen) = self.frozen_input.lock() {
            let (abandoned, retained): (Vec<_>, Vec<_>) = frozen.drain(..)
                .partition(|route| route.client == self.client);
            *frozen = retained.into();
            abandoned
        } else { Vec::new() };
        for route in abandoned {
            let _ = self.input_recovery.finish(self.client, route.route.delivery,
                XAuthorityInputDeliveryOutcome::ClientDisconnected);
        }
    }

    /// Take back the reservation of a connection that was never exposed.
    fn relinquish_unexposed(&self) {
        let taken = match self.ordered_continuation.lock() {
            Ok(mut held) => held.take(),
            Err(poisoned) => poisoned.into_inner().take(),
        };
        if let Some(unexposed) = taken {
            unexposed.relinquish_unexposed();
        }
    }
}
