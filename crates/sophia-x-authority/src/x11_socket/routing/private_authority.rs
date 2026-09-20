// The one authority an instance executes against, and the roles that reach it.
//
// Split by subject: who may reserve, who may execute, and who may take a
// reservation back are three different rights over one instance, and the
// point of this file is that they are not the same handle.

/// How many disposal debts a controller reserves room for.
///
/// A grant holds at most one request cell, so at most one token per grant can
/// be owed at once -- but that alone does not bound the number of grants. The
/// bound comes from the authority: its holder width is verified at
/// construction and each grant carries a nonzero device allowance, so the
/// grants it can issue are limited and this is built above that limit. Both
/// facts are needed; one-cell-per-grant without the grant bound says nothing
/// about how many debts can exist at once.
///
/// Recording a debt therefore never allocates, which matters because it
/// happens while common is held, on the path where something already failed.
#[cfg(unix)]
const PRIVATE_OWED_DISPOSAL: usize = 64;

/// The single authoritative instance, reachable only through role-limited
/// methods.
///
/// Shared rather than owned outright. The reservation API needs
/// `&mut AuthorityInstance`, and reserving has to happen at submission, which
/// is a detached producer -- so an instance held exclusively by the executor
/// could never be reserved against. Sharing it is not a rank inversion: a rank
/// orders acquisitions made while another guard is held, and a producer taking
/// this mutex holds nothing else when it does.
///
/// What is withheld is not the instance but the rights over it. Issuer rights
/// stay here, because a caller holding those could revoke and reissue the
/// grant its own request was validated against, and nothing downstream would
/// see that the request it is executing answers to a grant that no longer
/// exists.
#[cfg(unix)]
#[derive(Clone)]
pub struct PrivateAuthorityController {
    common: Arc<Mutex<sophia_input_authority::AuthorityInstance>>,
    /// Held, never handed out.
    issuer: Arc<sophia_input_authority::IssuerHandle>,
    /// Cells owed disposal that could not be disposed when they were dropped.
    ///
    /// Ranked beneath common, not outside the order: it is taken while common
    /// is held, and never the other way round. A reservation dropped while
    /// this thread already holds common cannot take common again to dispose
    /// itself -- that is a deadlock rather than a rank question -- so it
    /// records the debt here and the next caller holding common pays it.
    ///
    /// Its storage is reserved at construction, so recording a debt never
    /// allocates. The bound is the reason it can be: a grant holds at most one
    /// request cell, so at most one token per grant can ever be owed at once,
    /// and this is built above that.
    owed_disposal: Arc<Mutex<Vec<sophia_input_authority::RequestToken>>>,
}

/// Why a role-limited authority call could not be made.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivateAuthorityRefusal {
    /// A previous attempt entered execution without returning a guarded
    /// completion or effect-free deferral. It cannot be tried again.
    RequestUnresolved,
    /// Nothing currently admitted answers for this work. Not a refusal by the
    /// authority: the question of who is admitted was asked and came back
    /// empty, which is different from an authority that declined.
    NoCurrentAdmission,
    /// The frontend still owns its full configured client inventory. Retry
    /// only after a closing client completes lifecycle cleanup.
    ClientRecordsExhausted,
    /// The authority itself refused, before any effect.
    Authority(sophia_input_authority::RegistrationError),
    /// The instance could not be reached at all. Not an outcome: nothing was
    /// attempted, so nothing can be concluded about the work.
    Unreachable,
}

#[cfg(unix)]
impl PrivateAuthorityController {
    /// Build a controller over one authority and the issuer rights for it.
    ///
    /// Fallible, and checked rather than assumed. An instance paired with
    /// another instance's issuer accepts reservations -- the submit handle
    /// only establishes which authority is being addressed -- and then cannot
    /// dispose them, because disposal is an issuer act and that issuer answers
    /// for something else. The cell is stranded with nothing able to publish,
    /// consume or reissue it. The parts come back intact on refusal, since
    /// they may be the caller's only handles.
    #[allow(clippy::result_large_err)]
    pub fn new(
        authority: sophia_input_authority::AuthorityInstance,
        issuer: sophia_input_authority::IssuerHandle,
    ) -> Result<
        Self,
        (
            PrivateAuthorityRefusal,
            sophia_input_authority::AuthorityInstance,
            sophia_input_authority::IssuerHandle,
        ),
    > {
        if let Err(error) = authority.authority_identity(&issuer) {
            return Err((PrivateAuthorityRefusal::Authority(error), authority, issuer));
        }
        Ok(Self {
            common: Arc::new(Mutex::new(authority)),
            issuer: Arc::new(issuer),
            owed_disposal: Arc::new(Mutex::new(Vec::with_capacity(PRIVATE_OWED_DISPOSAL))),
        })
    }

    /// Act on the instance under common, paying any deferred disposal first.
    ///
    /// The caller takes whatever later-ranked guards it needs inside, which is
    /// the only order that works: common first, then X. Handing back a guard
    /// instead would let a caller hold common across something this file
    /// cannot see.
    fn under_common<R>(
        &self,
        act: impl FnOnce(&mut sophia_input_authority::AuthorityInstance) -> R,
    ) -> Result<R, PrivateAuthorityRefusal> {
        let mut held = self
            .common
            .lock()
            .map_err(|_| PrivateAuthorityRefusal::Unreachable)?;
        self.pay_owed_disposal(&mut held);
        Ok(act(&mut held))
    }

    /// Dispose everything recorded as owed, while common is already held.
    ///
    /// Reaches through poison for the same reason the debt exists at all: a
    /// skipped payment is a cell nobody can publish, consume or reissue, and a
    /// poisoned list of tokens is still a readable list of tokens. Declining
    /// here would make "deferred" mean "dropped" in exactly the case the defer
    /// was for.
    fn pay_owed_disposal(&self, authority: &mut sophia_input_authority::AuthorityInstance) {
        let mut owed = self
            .owed_disposal
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while let Some(token) = owed.pop() {
            let _ = authority.abandon_request(&self.issuer, token);
        }
    }

    /// Which authority this is. Read-only, so no role is implied by asking.
    pub fn identity(
        &self,
    ) -> Result<sophia_input_authority::AuthorityIdentity, PrivateAuthorityRefusal> {
        self.under_common(|authority| {
            authority
                .authority_identity(&self.issuer)
                .map_err(PrivateAuthorityRefusal::Authority)
        })?
    }

    /// Take back a reservation that was never published.
    ///
    /// Issuer-owned and exact: this disposes the one cell named, never a
    /// sweep. It never waits for common. If common cannot be taken without
    /// waiting -- including because this very thread holds it -- the debt is
    /// recorded and paid by the next caller that holds it, so a drop inside an
    /// execution callback records rather than deadlocks.
    fn dispose_unpublished(&self, token: sophia_input_authority::RequestToken) {
        match self.common.try_lock() {
            Ok(mut authority) => {
                self.pay_owed_disposal(&mut authority);
                let _ = authority.abandon_request(&self.issuer, token);
            }
            Err(std::sync::TryLockError::Poisoned(poisoned)) => {
                let mut authority = poisoned.into_inner();
                self.pay_owed_disposal(&mut authority);
                let _ = authority.abandon_request(&self.issuer, token);
            }
            Err(std::sync::TryLockError::WouldBlock) => {
                // Recorded even if the list itself is poisoned: a poisoned
                // list of tokens is still a readable list of tokens, and
                // declining here strands the cell this exists to release.
                let mut owed = self
                    .owed_disposal
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                owed.push(token);
            }
        }
    }

    /// Run final validation and application for one request this producer
    /// holds custody of.
    ///
    /// Act on the instance under common. Origin-only, and not `pub`.
    ///
    /// Common is acquired here and the caller does the rest inside: this is
    /// the only place the ranked adapter guards can be taken after common
    /// rather than before it. A method that acquired common itself and took a
    /// by-value identity read elsewhere would not implement that order -- it
    /// would require the caller to hold the adapter guards first, which is the
    /// forbidden direction, or to have dropped them, which makes what it read
    /// stale.
    ///
    /// Not offered to producers. Nothing that owns a reservation may be
    /// dropped inside, and nothing here hands one in.
    fn under_common_as_origin<R>(
        &self,
        act: impl FnOnce(
            &mut sophia_input_authority::AuthorityInstance,
            &sophia_input_authority::IssuerHandle,
        ) -> R,
    ) -> Result<R, PrivateAuthorityRefusal> {
        self.under_common(|authority| act(authority, &self.issuer))
    }
}

/// Why an instance would not hand out its keyboard state.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivateKeyboardsRefusal {
    /// The authority could not be read, so the state could not be bound to the
    /// instance it would answer for. Not a keymap problem: nothing was
    /// compiled and nothing was diagnosed about the keymap.
    AuthorityUnreadable,
    /// The keymap does not compile at all.
    KeymapUnavailable,
    /// This instance has already handed out its keyboard state.
    ///
    /// There is one keyboard history per instance and it is the state that has
    /// been accumulating. A second object would bind to the same authority and
    /// pass every identity check while holding none of what is currently held,
    /// so a key down in the first would be a key nobody released as far as the
    /// second is concerned.
    AlreadyIssued,
}

/// Names the instance and the seats, never the keymap state itself.
///
/// Hand-written because the states inside are a C library's, and formatting
/// one would put a keymap's internals into any log that prints a turn.
#[cfg(unix)]
impl std::fmt::Debug for PrivateKeyboards {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PrivateKeyboards")
            .field("authority", &self.authority)
            .field("seats", &self.seats.len())
            .finish_non_exhaustive()
    }
}

/// The keyboard state one executing thread owns.
///
/// Held by the thread that drives execution rather than by the frontend, and
/// deliberately so. The frontend is moved between threads today -- it is sent
/// into one to run a turn and comes back -- so putting keyboard state inside
/// it would either make it thread-bound without saying so, or need an unsafe
/// claim that a C library's state may cross threads. This says instead that
/// the state belongs to whoever is executing, which is what "on the executing
/// thread" has to mean to be worth anything.
///
/// The shared alternative is a worker thread reached by sending a command and
/// blocking on a reply with a deadline. That is a wait, and the routing path
/// takes it while already holding a guard; nothing may wait under the
/// execution guards, so an ordered execution cannot use it.
#[cfg(unix)]
pub struct PrivateKeyboards {
    /// Which private instance this state answers for.
    ///
    /// Being owned by the executing thread says nothing about *whose* state it
    /// is. Without this, one instance's turn could be driven with another
    /// instance's keyboard state and every modifier would be read from the
    /// wrong history.
    authority: sophia_input_authority::AuthorityIdentity,
    /// The keymap every seat here is built from, so a seat met later is built
    /// the same way as one met first.
    config: crate::XkbRmlvoConfig,
    seats: BTreeMap<SeatId, crate::XkbKeyboardState>,
}

#[cfg(unix)]
impl PrivateKeyboards {
    /// Build the state one instance's executing thread will own.
    ///
    /// The probe here compiles a keymap and discards it. That is worth doing
    /// -- a keymap that cannot compile at all is better found now than inside
    /// a transaction -- but it is worth exactly that and no more: it does not
    /// retain the compiled keymap, and it does not establish that a seat
    /// prepared later will compile successfully. Each seat compiles its own,
    /// and each can fail, which is why preparing is fallible and happens
    /// before anything depending on it is accepted.
    fn for_instance(
        authority: sophia_input_authority::AuthorityIdentity,
        config: crate::XkbRmlvoConfig,
    ) -> Result<Self, PrivateKeyboardsRefusal> {
        crate::XkbKeyboardState::new(&config)
            .map_err(|_| PrivateKeyboardsRefusal::KeymapUnavailable)?;
        Ok(Self {
            authority,
            config,
            seats: BTreeMap::new(),
        })
    }

    /// Whether this state answers for that instance.
    ///
    /// Asked before a turn uses it, so one instance cannot be driven with
    /// another's keyboard history.
    ///
    /// Necessary and not sufficient on its own: a second state built for the
    /// same instance would pass this while holding none of the keys the first
    /// is holding. What makes it sufficient is that an instance hands its
    /// state out once, so no second one exists to ask.
    pub fn answers_for(&self, authority: sophia_input_authority::AuthorityIdentity) -> bool {
        self.authority == authority
    }

    /// Make sure this seat has state, before any transaction is entered.
    ///
    /// Separate from applying, because building compiles a keymap and that is
    /// neither free nor infallible. Doing it inside the guards would put a
    /// fallible compilation where a refusal is no longer free.
    ///
    /// A seat already prepared is left exactly as it is. Rebuilding one would
    /// discard the modifiers it is holding, so a key held across the rebuild
    /// would be released by nobody and held by nothing.
    pub fn prepare(&mut self, seat: SeatId) -> bool {
        if self.seats.contains_key(&seat) {
            return true;
        }
        match crate::XkbKeyboardState::new(&self.config) {
            Ok(state) => {
                self.seats.insert(seat, state);
                true
            }
            Err(_) => false,
        }
    }

    /// Apply one key to a seat this thread has already prepared.
    ///
    /// Crate-internal on purpose. Mutating the seat's history is reachable
    /// only through guarded execution: a public call would let the live path
    /// move a seat's modifiers without the validation that decides whether the
    /// key may be applied at all, and the state would then describe keys no
    /// admitted request ever pressed.
    ///
    /// Returns the X keycode, the modifier state *before* the key, and the
    /// state after it. Both are needed and they are different facts: X reports
    /// the pre-event modifiers on the event itself, while what follows has to
    /// see the state the key produced.
    ///
    /// `None` where the seat was never prepared or the key does not map.
    /// Nothing is built here, so this cannot fail for want of a keymap.
    ///
    /// Its only production caller is the guarded execution path, which does
    /// not exist yet; until it does, nothing outside tests reaches this, which
    /// is the state the visibility is meant to produce.
    #[cfg_attr(not(test), allow(dead_code))]
    fn apply(&mut self, seat: SeatId, keycode: u32, pressed: bool) -> Option<(u8, u16, u16)> {
        let state = self.seats.get_mut(&seat)?;
        let (mapped, before) = state.map_evdev_key(keycode, pressed)?;
        Some((mapped, before, state.modifier_mask()))
    }

    /// The X keycode an evdev code maps to, without touching any state.
    ///
    /// Separated from applying because the ledger needs the keycode to name
    /// the input it is validating, and validation has to finish before any
    /// keyboard effect. Deriving it is arithmetic on the code itself and
    /// depends on no seat, so asking costs nothing and changes nothing.
    pub fn x_keycode(evdev_keycode: u32) -> Option<u8> {
        evdev_keycode
            .checked_add(8)
            .and_then(|keycode| u8::try_from(keycode).ok())
            .filter(|keycode| *keycode >= 8)
    }

    /// What this seat's modifiers are now, without applying anything.
    pub fn modifiers(&self, seat: SeatId) -> Option<u16> {
        self.seats.get(&seat).map(crate::XkbKeyboardState::modifier_mask)
    }
}

/// A reservation that has not reached the order yet.
///
/// Held rather than returned as a bare token, because the window between
/// reserving and publishing has exactly two honest ends: the work is published
/// and the reservation belongs to it, or it is not and the reservation goes
/// back. A bare token makes the second end something a caller has to remember
/// on every refusal path, including the ones that unwind.
///
/// There is deliberately no public way to extract the token. Publication is
/// internal and happens only where the work is genuinely accepted, so a
/// producer cannot disable disposal by asserting that it published.
#[cfg(unix)]
pub struct PrivateReservation {
    input_completion: Option<PrivateAcceptedInputCompletion>,
    controller: PrivateAuthorityController,
    /// Taken when the work reaches the order. `None` afterwards, so the drop
    /// below knows an unpublished reservation from one that has an owner.
    token: Option<sophia_input_authority::RequestToken>,
    connection: sophia_input_authority::ConnectionIdentity,
    capability: sophia_input_authority::DeviceCapability,
    submit: sophia_input_authority::SubmitHandle,
    admission: sophia_protocol::ClientAdmissionId,
    grant: sophia_input_authority::GrantId,
    /// Where this request's internal-processing outcome is reported, if
    /// anyone asked to be told. Travels with the work rather than being
    /// looked up, so publishing needs no registry and no search.
    barrier: Option<PrivateBarrierTicket>,
}

#[cfg(unix)]
impl PrivateReservation {
    pub fn capability(&self) -> sophia_input_authority::DeviceCapability {
        self.capability
    }

    /// The work reached the order, so custody of the request travels with it.
    ///
    /// Crate-internal and consuming: the only caller is the point where
    /// acceptance actually happened, and what comes back is custody rather
    /// than a bare token.
    #[cfg_attr(not(test), allow(dead_code))]
    fn accepted(mut self) -> PrivateOutstandingRequest {
        let token = self
            .token
            .take()
            .expect("a reservation reaches the order at most once");
        PrivateOutstandingRequest {
            input_completion: self.input_completion.take(),
            controller: self.controller.clone(),
            submit: self.submit,
            token,
            connection: self.connection,
            admission: self.admission,
            grant: self.grant,
            capability: self.capability,
            observed: std::cell::Cell::new(false),
            observed_outcome: std::cell::Cell::new(None),
            accepted_store_credit: None,
            phase: std::cell::Cell::new(PrivateRequestPhase::Unused),
            barrier: self.barrier.take(),
        }
    }
}

/// Names the request and nothing about the authority behind it.
///
/// Hand-written because the controller holds an instance whose debug would
/// print an authority's internals into any log that formats an envelope.
#[cfg(unix)]
impl std::fmt::Debug for PrivateReservation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PrivateReservation")
            .field("published", &self.token.is_none())
            .finish_non_exhaustive()
    }
}

#[cfg(unix)]
impl Drop for PrivateReservation {
    fn drop(&mut self) {
        let Some(token) = self.token.take() else {
            return;
        };
        // Never published, so the cell it holds answers to nothing. Disposed
        // through the origin's issuer rights rather than by the producer,
        // which has none, and by exact token rather than by clearing whatever
        // the grant currently holds -- that cell may already belong to the
        // next request.
        self.controller.dispose_unpublished(token);
    }
}

/// Custody of one request that reached the order.
///
/// This is what makes observing an outcome a right rather than a guess. The
/// token and the connection are carried here, not supplied by whoever calls,
/// so a producer cannot name another producer's request and consume its
/// completion -- which the authority alone cannot prevent, because a shared
/// submit handle establishes only which authority is being addressed and the
/// connection test compares against what the caller passed in.
#[cfg(unix)]
pub struct PrivateOutstandingRequest {
    /// Captured atomically with recovery admission before queue publication.
    /// A thaw may not adopt a replacement cell for the same delivery id.
    input_completion: Option<PrivateAcceptedInputCompletion>,
    /// The prepared order's accepted-item charge, never released by Drop.
    /// Weak ownership prevents store -> terminal -> store cycles.
    accepted_store_credit: Option<PrivateAcceptedItemCredit>,
    controller: PrivateAuthorityController,
    submit: sophia_input_authority::SubmitHandle,
    token: sophia_input_authority::RequestToken,
    /// The connection this request was reserved for.
    ///
    /// The right identity to observe an outcome against, and the wrong one to
    /// execute on: by execution time it is old evidence, and comparing it
    /// against itself would pass for a client that has since gone.
    connection: sophia_input_authority::ConnectionIdentity,
    /// The admission this request's grant was issued under.
    admission: sophia_protocol::ClientAdmissionId,
    /// The grant that owns this request, for authorising its settlement.
    grant: sophia_input_authority::GrantId,
    /// The capability this request was reserved under.
    ///
    /// Carried rather than looked up again at execution. A native operation
    /// validates that the capability and the permit name the same source, and
    /// a capability fetched later would be whatever the producer holds now
    /// rather than the one this request was accepted against.
    capability: sophia_input_authority::DeviceCapability,
    /// Whether the terminal outcome has been taken.
    ///
    /// Execution alone does not free the cell -- the completion has to be
    /// observed -- so custody that ends without observing owes the cell back.
    observed: std::cell::Cell<bool>,
    /// The actual consumed outcome remains readable if item disposal is
    /// interrupted after common has released its request cell.
    observed_outcome: std::cell::Cell<Option<sophia_input_authority::RequestCompletion>>,
    /// How far this request got.
    ///
    /// An unexposed reservation, an interrupted attempt, a proved effect-free
    /// deferral and a terminal result have different retention obligations.
    phase: std::cell::Cell<PrivateRequestPhase>,
    /// Where this request's outcome is reported, if anyone is waiting.
    barrier: Option<PrivateBarrierTicket>,
}

/// How far a reserved request got.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrivateRequestPhase {
    /// Reserved and never entered. Its cell holds nothing, so taking it back
    /// costs nobody an answer.
    Unused,
    /// Execution was entered and did not return. Whether it had an effect is
    /// exactly what was lost, so this is neither discarded nor replayed nor
    /// relabelled as an outcome. An absent completion is not proof that
    /// nothing ran.
    Entered,
    /// Common explicitly returned without consuming the permit or applying
    /// an effect. The accepted original request remains outstanding; losing
    /// its adapter owner must not silently dispose of that reservation.
    DeferredBeforeEffect,
    /// Execution returned. The cell holds a terminal outcome, and reclaiming
    /// the slot would erase what happened.
    Settled,
}

#[cfg(unix)]
impl PrivateOutstandingRequest {
    fn input_completion(&self) -> Option<&PrivateAcceptedInputCompletion> {
        self.input_completion.as_ref()
    }

    /// The capability this request was reserved under.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn capability(&self) -> sophia_input_authority::DeviceCapability {
        self.capability
    }

    /// The request this custody answers for. Crate-internal: naming it is not
    /// a right, holding this value is.
    fn token(&self) -> sophia_input_authority::RequestToken {
        self.token
    }

    /// The admission this request's grant answers to.
    fn admission(&self) -> sophia_protocol::ClientAdmissionId {
        self.admission
    }

    /// The grant that owns this request.
    fn grant(&self) -> sophia_input_authority::GrantId {
        self.grant
    }

    /// The client this request was reserved for.
    ///
    /// Carried rather than looked up: knowing which client a request belongs
    /// to must not require a guard, or establishing it would have to happen
    /// before common and be stale by the time execution used it.
    fn client(&self) -> XServerFrontendClientId {
        XServerFrontendClientId::from_raw(self.connection.recipient)
    }

    /// Mark that execution is being entered, before anything inside it can
    /// take effect or unwind.
    ///
    /// Written ahead rather than after, for the same reason every other step
    /// in this design is: a marker set once the call returns says nothing
    /// about a call that did not. An interruption after this leaves a request
    /// whose effect is unknown, which is a state to preserve rather than a
    /// request that never ran.
    fn entering(&self) {
        self.phase.set(PrivateRequestPhase::Entered);
    }

    /// Record that execution returned, whatever the outcome was.
    ///
    /// Recorded for a refusal as well as a success: a request refused after
    /// its effect was marked carries a real outcome, and that is exactly the
    /// one that must not be erased to reclaim a slot.
    fn settled(&self) {
        self.phase.set(PrivateRequestPhase::Settled);
    }

    /// Only the authority's guarded Deferred answer can make this transition.
    fn deferred_before_effect(&self) {
        self.phase.set(PrivateRequestPhase::DeferredBeforeEffect);
    }

    /// The consumer refused this request before the authority ran it.
    ///
    /// Published through the reserved execution as the request's completion,
    /// exactly as a refusal the authority itself makes is, and then observed
    /// here: that frees the grant's one cell at once, wakes whoever waits on
    /// the request, and leaves the outcome for the terminal to answer the
    /// delivery with. Without this a request the executor declined before
    /// entering the authority had no completion at all, and the grant stayed
    /// busy for good: one refused injection wedged its producer.
    ///
    /// Only a request that never entered the authority is refused this way;
    /// one that entered has a completion of its own coming.
    pub(crate) fn refuse_unexecuted(&self) -> Result<(), PrivateAuthorityRefusal> {
        if self.phase.get() != PrivateRequestPhase::Unused {
            return Ok(());
        }
        self.controller.under_common_as_origin(|authority, issuer| {
            authority
                .execute_reserved(issuer, self.token, self.connection, |_permit| {
                    Err(sophia_input_authority::RegistrationError::ConsumerRefused)
                })
                .map(|_completion| ())
                .map_err(PrivateAuthorityRefusal::Authority)
        })??;
        self.observe().map(|_observed| ())
    }

    /// Observe the outcome of this request, and only this one.
    ///
    /// This is what frees the grant's one cell, so the next request on it can
    /// be reserved. Executing does not: it produces the outcome, and the
    /// outcome sits in the cell until somebody takes it.
    pub fn observe(
        &self,
    ) -> Result<Option<sophia_input_authority::RequestCompletion>, PrivateAuthorityRefusal> {
        let taken = self.controller.under_common(|authority| {
            authority
                .take_completion(&self.submit, self.token, self.connection)
                .map_err(PrivateAuthorityRefusal::Authority)
        })?;
        if let Ok(Some(outcome)) = taken {
            // Recorded only for an outcome that was actually taken. A call
            // that found nothing waiting has freed nothing, and treating it as
            // observation would leave the cell held with nothing left to
            // release it.
            self.observed.set(true);
            self.observed_outcome.set(Some(outcome));
            // The second of two publication points, and the one that is not
            // optional. A cancellation written by a revocation or a control
            // transition never runs the execution callback at all, so this is
            // where it first becomes readable. First writer wins, so a
            // request already answered by its own execution keeps that answer
            // instead of having it replaced by the cell being reclaimed.
            self.report(outcome);
        }
        taken
    }

    /// Store this request's outcome for whoever is waiting on it.
    ///
    /// Called under the common guard, where the authority permits nothing
    /// fallible after the effect, so this only ever copies into a slot that
    /// already exists.
    fn report(&self, completion: sophia_input_authority::RequestCompletion) {
        if let Some(ticket) = &self.barrier {
            ticket.report(completion);
        }
    }

    /// Raise the wake for an outcome this request already stored.
    ///
    /// Never under the common guard: notification routing stays outside the
    /// critical section, and a wake is one write to an eventfd that is
    /// allowed to fail. Failing means the waiter departed, which costs
    /// nothing, because the outcome is committed where a waiter would look.
    pub(crate) fn flush_report(&self) {
        if let Some(ticket) = &self.barrier {
            ticket.flush();
        }
    }
}

#[cfg(unix)]
impl Drop for PrivateOutstandingRequest {
    fn drop(&mut self) {
        if self.observed.get() {
            // Already released by the observation that took its outcome.
            return;
        }
        match self.phase.get() {
            // Reserved and never entered. The cell holds nothing, so taking it
            // back costs nobody an outcome and leaving it costs the grant its
            // only one.
            PrivateRequestPhase::Unused => self.controller.dispose_unpublished(self.token),
            // Entered without returning, guarded deferral of accepted work,
            // or returned with an outcome. None
            // may be discarded to reclaim a slot: one holds a terminal outcome
            // that discarding would erase, and the other holds a question
            // nobody can answer -- and answering it by removing the record
            // turns "unknown" into "never happened". The authority's cleanup
            // is for a connection that departed, and losing a handle does not
            // establish that. Retiring either is a separate act by whoever can
            // establish the departure or the settlement, and until then the
            // cell costs capacity rather than an answer.
            PrivateRequestPhase::Entered
            | PrivateRequestPhase::DeferredBeforeEffect
            | PrivateRequestPhase::Settled => {}
        }
    }
}

/// A producer's role over the authority: its own reservations, and nothing
/// else.
///
/// Cloneable and detached on purpose. Two producers contending for one order
/// is the thing being built, so this has to survive being handed to one.
#[cfg(unix)]
#[derive(Clone)]
pub struct PrivateReservationRole {
    controller: PrivateAuthorityController,
    submit: sophia_input_authority::SubmitHandle,
    capability: sophia_input_authority::DeviceCapability,
    /// Bound at issue, not supplied per call. A producer asked to provide
    /// these would have to obtain them from somewhere, and the only places
    /// they exist are this role and another role's.
    generation: sophia_input_authority::GrantGeneration,
    connection: sophia_input_authority::ConnectionIdentity,
    /// The grant this role's capability was issued under.
    ///
    /// Carried because settling a debt names the participant that owes it, and
    /// the capability does not expose its grant outside the authority. A
    /// settlement offered without it cannot be authorised, and the debt stays
    /// open on work that was delivered.
    grant: sophia_input_authority::GrantId,
    /// The admission this role was issued under.
    ///
    /// Carried so execution can tell a replacement admission from the one its
    /// grant answers to. A generation alone cannot: a client readmitted under
    /// the same session generation is a different admission, and matching on
    /// the number would make an old grant current again.
    admission: sophia_protocol::ClientAdmissionId,
}

#[cfg(unix)]
impl PrivateReservationRole {
    fn new(
        controller: PrivateAuthorityController,
        submit: sophia_input_authority::SubmitHandle,
        capability: sophia_input_authority::DeviceCapability,
        generation: sophia_input_authority::GrantGeneration,
        connection: sophia_input_authority::ConnectionIdentity,
        admission: sophia_protocol::ClientAdmissionId,
        grant: sophia_input_authority::GrantId,
    ) -> Self {
        Self {
            controller,
            submit,
            capability,
            generation,
            connection,
            admission,
            grant,
        }
    }

    pub fn connection(&self) -> sophia_input_authority::ConnectionIdentity {
        self.connection
    }

    /// Reserve one request, before the work is published.
    ///
    /// The caller supplies the stamp it already captured through the
    /// coordinator and released, plus which request this is. The grant
    /// generation and the connection come from this role, so a producer never
    /// has to obtain an opaque value it has no way to know -- and never has a
    /// reason to reach for another role's.
    ///
    /// A transition landing between the stamp and this call is caught by the
    /// authority's own validation rather than by a check racing it, and
    /// nothing here reaches back for the coordinator from under common.
    pub fn reserve(
        &self,
        stamp: crate::ControlStamp,
        request: u64,
    ) -> Result<PrivateReservation, PrivateAuthorityRefusal> {
        let context = sophia_input_authority::ExecutionContext {
            generation: self.generation,
            connection: self.connection,
            epoch: stamp.control_epoch,
            publication: stamp.publication,
            request,
        };
        let token = self.controller.under_common(|authority| {
            authority
                .reserve_request(&self.submit, self.capability, context)
                .map_err(PrivateAuthorityRefusal::Authority)
        })??;
        Ok(PrivateReservation {
            input_completion: None,
            controller: self.controller.clone(),
            token: Some(token),
            connection: self.connection,
            capability: self.capability,
            submit: self.submit,
            admission: self.admission,
            grant: self.grant,
            // Attached by the producer that accepted the work, which is the
            // only place that knows whether anyone is waiting on it.
            barrier: None,
        })
    }
}
