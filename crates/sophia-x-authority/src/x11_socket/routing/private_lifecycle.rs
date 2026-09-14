// Connection cleanup custody. This owner must travel with the private
// terminal inventory, including when that inventory has no input holds.
// Register before exposing ingress; install the returned gate in admission
// checks. Merely holding this owner does not wire those checks into Session.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PrivateLifecycleRefusal {
    Unreachable,
    Capacity,
    IdentityExhausted,
    DifferentAdmission,
    AlreadyOwned,
    NotAdmitted,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct PrivateLifecycleInventory {
    pub open: usize,
    pub closed: usize,
    pub native_unknown: usize,
    /// Connections whose exact grant retirement has not yet been verified.
    pub retirement_checks_pending: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PrivateLifecycleIdentity {
    client: XServerFrontendClientId,
    admission: sophia_protocol::ClientAdmissionId,
    generation: u64,
    namespace: NamespaceId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrivateLifecycleNative {
    Pending,
    Started,
    Complete,
}

struct PrivateLifecycleRecord {
    identity: PrivateLifecycleIdentity,
    native: PrivateLifecycleNative,
    grants_pending: bool,
    leased: bool,
}

struct PrivateLifecycleSlot {
    mark: Arc<std::sync::atomic::AtomicU64>,
    incarnation: u64,
    record: Option<PrivateLifecycleRecord>,
}

struct PrivateLifecycleRecords {
    slots: Vec<PrivateLifecycleSlot>,
    cursor: usize,
}

struct PrivateLifecycleCore {
    participant: PrivateAdmissionParticipant,
    accepting: std::sync::atomic::AtomicBool,
    marks: Vec<Arc<std::sync::atomic::AtomicU64>>,
    // Retained for later native reconciliation, never cleared merely because
    // the last query client departed. Common/native holds may still exist.
    _pointers: Arc<Mutex<BTreeMap<(NamespaceId, SeatId), crate::XCorePointerMapper>>>,
    native: Arc<Mutex<crate::XInputAuthorityState>>,
    records: Mutex<PrivateLifecycleRecords>,
}

/// A shared owner, not a registry back-reference. Carry a clone in each
/// live/settlement/durable terminal inventory until inventory() is empty.
#[derive(Clone)]
#[must_use = "connection cleanup ownership must outlive the frontend"]
pub(crate) struct PrivateLifecycleOwner {
    inner: Arc<PrivateLifecycleCore>,
}

/// A check/close-request capability for one exact slot incarnation. It cannot
/// reopen an admission or report revocation complete. An execution already
/// holding common may finish; drive linearizes actual retirement under common.
#[derive(Clone, Debug)]
pub(crate) struct PrivateLifecycleGate {
    mark: Arc<std::sync::atomic::AtomicU64>,
    open: u64,
}

impl PrivateLifecycleGate {
    pub fn is_open(&self) -> bool {
        self.mark.load(Ordering::Acquire) == self.open
    }
    pub fn close(&self) {
        let _ = self.mark.compare_exchange(
            self.open,
            self.open | 1,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }
}

/// Query-owner custody. Drop only marks preallocated state and never takes a
/// mutex, allocates, enters common, or invokes native cleanup.
#[must_use]
pub(crate) struct PrivateConnectionLifecycle {
    owner: PrivateLifecycleOwner,
    gate: PrivateLifecycleGate,
}

impl PrivateConnectionLifecycle {
    pub fn gate(&self) -> PrivateLifecycleGate {
        self.gate.clone()
    }
    pub fn close(&self) {
        self.gate.close();
    }
    pub fn belongs_to(&self, owner: &PrivateLifecycleOwner) -> bool {
        Arc::ptr_eq(&self.owner.inner, &owner.inner)
    }
}

impl Drop for PrivateConnectionLifecycle {
    fn drop(&mut self) {
        self.gate.close();
    }
}

impl PrivateLifecycleOwner {
    /// Capacity is the frontend's configured admission limit, supplied by its
    /// constructor. No input queue, grant count, or arbitrary local default
    /// can bound ordinary clients that hold no synthetic grants.
    fn prepare(
        max_concurrent_clients: NonZeroUsize,
    ) -> Result<PrivateLifecycleRecords, PrivateLifecycleRefusal> {
        let mut slots = Vec::new();
        slots
            .try_reserve_exact(max_concurrent_clients.get())
            .map_err(|_| PrivateLifecycleRefusal::Capacity)?;
        for _ in 0..max_concurrent_clients.get() {
            slots.push(PrivateLifecycleSlot {
                mark: Arc::new(std::sync::atomic::AtomicU64::new(1)),
                incarnation: 0,
                record: None,
            });
        }
        Ok(PrivateLifecycleRecords { slots, cursor: 0 })
    }

    fn from_prepared(
        participant: PrivateAdmissionParticipant,
        native: Arc<Mutex<crate::XInputAuthorityState>>,
        pointers: Arc<Mutex<BTreeMap<(NamespaceId, SeatId), crate::XCorePointerMapper>>>,
        records: PrivateLifecycleRecords,
    ) -> Self {
        let marks = records.slots.iter().map(|slot| slot.mark.clone()).collect();
        Self {
            inner: Arc::new(PrivateLifecycleCore {
                participant,
                accepting: std::sync::atomic::AtomicBool::new(true),
                marks,
                native,
                _pointers: pointers,
                records: Mutex::new(records),
            }),
        }
    }

    fn install(&self) -> Result<(), PrivateLifecycleRefusal> {
        self.inner
            .participant
            .lifecycle
            .set(Arc::downgrade(&self.inner))
            .map_err(|_| PrivateLifecycleRefusal::AlreadyOwned)
    }

    // Common and participant bindings are already held by the admission path.
    fn register_held(
        &self,
        client: XServerFrontendClientId,
        bound: &PrivateAdmissionBinding,
    ) -> Result<PrivateLifecycleGate, PrivateLifecycleRefusal> {
        if !self.inner.accepting.load(Ordering::Acquire) {
            return Err(PrivateLifecycleRefusal::NotAdmitted);
        }
        let mut held = self
            .inner
            .records
            .lock()
            .map_err(|_| PrivateLifecycleRefusal::Unreachable)?;
        if held.slots.iter().any(|slot| {
            slot.record
                .as_ref()
                .is_some_and(|record| record.identity.client == client)
        }) {
            return Err(PrivateLifecycleRefusal::AlreadyOwned);
        }
        let slot = held
            .slots
            .iter_mut()
            .find(|slot| slot.record.is_none())
            .ok_or(PrivateLifecycleRefusal::Capacity)?;
        let incarnation = slot
            .incarnation
            .checked_add(1)
            .filter(|id| *id <= u64::MAX >> 1)
            .ok_or(PrivateLifecycleRefusal::IdentityExhausted)?;
        let open = incarnation << 1;
        slot.incarnation = incarnation;
        slot.record = Some(PrivateLifecycleRecord {
            identity: PrivateLifecycleIdentity {
                client,
                admission: bound.admission,
                generation: bound.generation,
                namespace: bound.namespace,
            },
            native: PrivateLifecycleNative::Pending,
            grants_pending: true,
            leased: false,
        });
        slot.mark.store(open, Ordering::Release);
        let gate = PrivateLifecycleGate {
            mark: slot.mark.clone(),
            open,
        };
        if !self.inner.accepting.load(Ordering::Acquire) {
            gate.close();
        }
        Ok(gate)
    }

    /// Acquire the actual connection's cleanup custody, before writer exposure.
    pub fn register(
        &self,
        client: XServerFrontendClientId,
        admission: sophia_protocol::ClientAdmissionContext,
    ) -> Result<PrivateConnectionLifecycle, PrivateLifecycleRefusal> {
        self.inner
            .participant
            .under_boundary(|_, _, bindings| {
                let bound = bindings
                    .bound
                    .get(&client)
                    .filter(|bound| !bound.closed)
                    .ok_or(PrivateLifecycleRefusal::NotAdmitted)?;
                if bound.admission != admission.client_id
                    || bound.generation != admission.auth_provenance.session_generation
                    || bound.namespace != admission.namespace.id
                {
                    return Err(PrivateLifecycleRefusal::DifferentAdmission);
                }
                if bound.lifecycle.is_none() {
                    self.register_held(client, bound)?;
                }
                let mut held = self
                    .inner
                    .records
                    .lock()
                    .map_err(|_| PrivateLifecycleRefusal::Unreachable)?;
                let slot = held
                    .slots
                    .iter_mut()
                    .find(|slot| {
                        slot.record
                            .as_ref()
                            .is_some_and(|r| r.identity.client == client)
                    })
                    .ok_or(PrivateLifecycleRefusal::NotAdmitted)?;
                let record = slot.record.as_mut().expect("matched");
                if record.leased {
                    return Err(PrivateLifecycleRefusal::AlreadyOwned);
                }
                let gate = PrivateLifecycleGate {
                    mark: slot.mark.clone(),
                    open: slot.incarnation << 1,
                };
                if !gate.is_open() {
                    return Err(PrivateLifecycleRefusal::NotAdmitted);
                }
                record.leased = true;
                Ok(PrivateConnectionLifecycle {
                    owner: self.clone(),
                    gate,
                })
            })
            .map_err(|_| PrivateLifecycleRefusal::Unreachable)?
    }

    /// Instance shutdown requests all current and racing admissions to close.
    /// Pure atomic stores: no common, inventory mutex, or native work in Drop.
    fn close_all(&self) {
        self.inner.accepting.store(false, Ordering::Release);
        for mark in &self.inner.marks {
            mark.fetch_or(1, Ordering::AcqRel);
        }
    }

    fn register_query_gate(
        &self,
        gate: &PrivateLifecycleGate,
        namespace: NamespaceId,
        client: XServerFrontendClientId,
    ) -> Result<(), PrivateLifecycleRefusal> {
        self.inner
            .participant
            .under_boundary(|_, _, bindings| {
                let bound = bindings
                    .bound
                    .get(&client)
                    .filter(|b| !b.closed && b.namespace == namespace)
                    .ok_or(PrivateLifecycleRefusal::NotAdmitted)?;
                if !gate.is_open()
                    || bound.lifecycle.as_ref().is_none_or(|bound_gate| {
                        !Arc::ptr_eq(&bound_gate.mark, &gate.mark) || bound_gate.open != gate.open
                    })
                {
                    return Err(PrivateLifecycleRefusal::NotAdmitted);
                }
                self.inner
                    .native
                    .lock()
                    .map_err(|_| PrivateLifecycleRefusal::Unreachable)?
                    .register_query_client(namespace, client.raw());
                Ok(())
            })
            .map_err(|_| PrivateLifecycleRefusal::Unreachable)?
    }

    pub fn inventory(&self) -> Result<PrivateLifecycleInventory, PrivateLifecycleRefusal> {
        let held = self
            .inner
            .records
            .lock()
            .map_err(|_| PrivateLifecycleRefusal::Unreachable)?;
        let mut result = PrivateLifecycleInventory::default();
        for slot in &held.slots {
            if let Some(record) = &slot.record {
                if slot.mark.load(Ordering::Acquire) & 1 == 0 {
                    result.open += 1;
                } else {
                    result.closed += 1;
                }
                result.native_unknown +=
                    usize::from(record.native == PrivateLifecycleNative::Started);
                result.retirement_checks_pending += usize::from(record.grants_pending);
            }
        }
        Ok(result)
    }

    /// Common -> admission bindings -> owned records -> native
    /// input authority. Never takes outer runtime or waits for a worker. Each
    /// visited slot consumes budget, and the persistent cursor gives retained
    /// failures their turn without skipping the rest indefinitely.
    pub fn drive(&self, budget: NonZeroUsize) -> Result<usize, PrivateLifecycleRefusal> {
        self.inner
            .participant
            .under_boundary(|authority, issuer, bindings| {
                let mut held = self
                    .inner
                    .records
                    .lock()
                    .map_err(|_| PrivateLifecycleRefusal::Unreachable)?;
                let mut completed = 0;
                for _ in 0..budget.get().min(held.slots.len()) {
                    let index = held.cursor;
                    held.cursor = (index + 1) % held.slots.len();
                    let slot = &mut held.slots[index];
                    if slot.mark.load(Ordering::Acquire) & 1 == 0 {
                        continue;
                    }
                    let Some(record) = slot.record.as_mut() else {
                        continue;
                    };
                    let id = record.identity;
                    if bindings.bound.get(&id.client).is_some_and(|bound| {
                        bound.admission != id.admission
                            || bound.generation != id.generation
                            || bound.namespace != id.namespace
                    }) {
                        return Err(PrivateLifecycleRefusal::DifferentAdmission);
                    }
                    // This closes the binding first; unfinished grant retirements
                    // remain in the participant inventory, never in a temporary.
                    close_and_retire(authority, issuer, bindings, id.client);
                    record.grants_pending = bindings.bound.contains_key(&id.client);
                    if record.native == PrivateLifecycleNative::Pending {
                        let mut native = self
                            .inner
                            .native
                            .lock()
                            .map_err(|_| PrivateLifecycleRefusal::Unreachable)?;
                        // Written before the effect. An unwind here retains an
                        // unknown native remainder; blindly replaying it could
                        // clear a newer contribution. There is no done setter.
                        record.native = PrivateLifecycleNative::Started;
                        native.cleanup_owner(id.client.raw());
                        record.native = PrivateLifecycleNative::Complete;
                    }
                    if record.native == PrivateLifecycleNative::Complete && !record.grants_pending {
                        slot.record = None;
                        completed += 1;
                    }
                }
                Ok(completed)
            })
            .map_err(|_| PrivateLifecycleRefusal::Unreachable)?
    }
}

impl XServerFrontendRouteRegistry {
    fn attach_private_lifecycle(
        &self,
        registration: &XServerFrontendClientRouteRegistration,
        context: ClientAdmissionContext,
    ) -> Result<(), X11SetupSocketError> {
        let Some(owner) = self.input_recovery.lifecycle.get() else {
            return Ok(());
        };
        owner
            .inner
            .participant
            .admit(registration.client, context)
            .map_err(|error| {
                X11SetupSocketError::new(format!("private admission failed: {error:?}"))
            })?;
        let lease = owner
            .register(registration.client, context)
            .map_err(|error| {
                X11SetupSocketError::new(format!("private lifecycle failed: {error:?}"))
            })?;
        let clients = self
            .clients
            .lock()
            .map_err(|_| X11SetupSocketError::new("private client table unavailable"))?;
        let entry = clients
            .get(&registration.client)
            .ok_or_else(|| X11SetupSocketError::new("private client disappeared"))?;
        if !Arc::ptr_eq(&entry.connection_state, &registration.connection_state) {
            return Err(X11SetupSocketError::new("private registration replaced"));
        }
        self.input_recovery
            .attach_lifecycle(registration.client, lease.gate())
            .map_err(|error| {
                X11SetupSocketError::new(format!("private recovery binding failed: {error:?}"))
            })?;
        *registration
            .lifecycle
            .lock()
            .map_err(|_| X11SetupSocketError::new("private lifecycle lease unavailable"))? =
            Some(lease);
        Ok(())
    }
}
