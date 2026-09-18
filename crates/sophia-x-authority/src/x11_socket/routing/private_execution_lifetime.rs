/// Availability of the original execution history, separate from settlement.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivateExecutionAvailability {
    Executing,
    /// The original thread still owns the history and its accounting state.
    Retained,
    /// The execution owner ended. Unresolved obligations remain owed, but
    /// their original history cannot be reconstructed or resumed.
    Abandoned,
}

/// An invocation's execution availability. Instance numbers are scoped to
/// the service's durable owner; they are not a transferable authorization.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrivateExecutionReading {
    pub instance: u64,
    pub availability: PrivateExecutionAvailability,
}

#[cfg(unix)]
struct PrivateExecutionWitness {
    instance: u64,
    state: std::sync::atomic::AtomicU8,
    /// Published only after the real settlement transferred all its fields.
    handed_off: AtomicBool,
    /// Positive same-invocation completion, separate from resource availability.
    completed: AtomicBool,
}

#[cfg(unix)]
impl PrivateExecutionWitness {
    fn reading(&self) -> PrivateExecutionReading {
        PrivateExecutionReading {
            instance: self.instance,
            availability: match self.state.load(Ordering::Acquire) {
                0 => PrivateExecutionAvailability::Executing,
                1 => PrivateExecutionAvailability::Retained,
                _ => PrivateExecutionAvailability::Abandoned,
            },
        }
    }
}

/// A durable handle to one invocation's execution witness.
///
/// READABLE AFTER THE KEEPER IS GONE, WHICH IS THE WHOLE POINT.
/// [`PrivateServiceExecutionKeeper::execution`] is a snapshot taken while the
/// keeper still exists, so a reading taken before the keeper drops reports the
/// execution as retained no matter what happens to it afterwards. Reporting
/// that as an outcome makes a joined thread look like a live execution.
///
/// This handle outlives both the keeper and the thread it ran on. When the
/// keeper drops, its lifetime owner publishes abandonment into this same
/// witness, so a reader that joins the thread first and reads afterwards sees
/// what is true after the join instead of what was true before it.
#[cfg(unix)]
#[derive(Clone)]
pub struct PrivateExecutionWitnessHandle(Arc<PrivateExecutionWitness>);

#[cfg(unix)]
impl PrivateExecutionWitnessHandle {
    /// The availability as it stands now, not as it stood when taken.
    pub fn reading(&self) -> PrivateExecutionReading {
        self.0.reading()
    }

    /// Positive completion of the invocation this witness belongs to.
    pub fn completed(&self) -> bool {
        self.0.completed.load(Ordering::Acquire)
    }
}

#[cfg(unix)]
impl core::fmt::Debug for PrivateExecutionWitnessHandle {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("PrivateExecutionWitnessHandle")
            .field("reading", &self.reading())
            .finish()
    }
}

// The terminal inventory owns only a witness. It cannot keep the executor
// alive, form a cycle, or move XKB state to another thread. This owner's
// destruction publishes loss even when a thread unwinds.
#[cfg(unix)]
struct PrivateExecutionLifetimeOwner(Arc<PrivateExecutionWitness>);

#[cfg(unix)]
impl Drop for PrivateExecutionLifetimeOwner {
    fn drop(&mut self) {
        self.0.state.store(2, Ordering::Release);
    }
}

/// Keeps one invocation's original execution resources on its executing
/// thread after service collection, including after an unwind. Construct it
/// outside the service's catch_unwind scope. Retention does not authorize a
/// maintenance effect or establish that any obligation has settled.
///
/// Dropping this keeper makes its inventory's execution reading Abandoned;
/// the independently owned unresolved inventory is preserved.
///
/// ```compile_fail
/// fn move_keeper(keeper: sophia_x_authority::PrivateServiceExecutionKeeper) {
///     std::thread::spawn(move || drop(keeper));
/// }
/// ```
#[cfg(unix)]
#[derive(Default)]
pub struct PrivateServiceExecutionKeeper {
    resources: Option<PrivateRetainedExecutionResources>,
    maintenance: PrivateMaintenanceCursor,
}

#[cfg(unix)]
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "Original running-loop preference and cursor remain retained for future cleanup work"
    )
)]
struct PrivateRetainedExecutionResources {
    // Publish abandonment before destroying the watch or native history.
    lifetime: PrivateExecutionLifetimeOwner,
    watch: Option<private_watchdog::PrivateWatchdogOwner>,
    origin: XServerFrontendRouteRegistry,
    /// Another handle to the exact prepared native origin, never a freshly
    /// prepared replacement. Holds still validate its original Arc identity.
    native_owner: private_native::Owner,
    /// The service collection's actual proof of ended connection frames.
    /// Kept only for this closed invocation; never reminted by maintenance.
    collected: Option<PrivateConnectionsCollected>,
    queue: Arc<Mutex<SharedQueue>>,
    keyboards: PrivateKeyboards,
    namespace: NamespaceId,
    seat: SeatId,
    service_origin: std::time::Instant,
    service: sophia_input_authority::ServiceBudget,
    prefer_cleanup: bool,
    reclaim_cursor: usize,
}

#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrivateExecutionBorrowRefusal {
    Unavailable,
    ForeignInvocation,
}

#[cfg(unix)]
impl PrivateServiceExecutionKeeper {
    pub fn new() -> Self {
        Self::default()
    }

    /// Availability of the invocation retained here, if service preparation
    /// reached an execution owner. A setup refusal leaves this empty.
    pub fn execution(&self) -> Option<PrivateExecutionReading> {
        self.resources
            .as_ref()
            .map(|resources| resources.lifetime.0.reading())
    }

    /// A handle to this invocation's witness that outlives the keeper.
    ///
    /// Taken while the keeper is alive and read after it is gone, so a caller
    /// that joins the serving thread can report what the execution is rather
    /// than what it was.
    pub fn execution_witness(&self) -> Option<PrivateExecutionWitnessHandle> {
        self.resources
            .as_ref()
            .map(|resources| PrivateExecutionWitnessHandle(Arc::clone(&resources.lifetime.0)))
    }

    /// Positive completion of this closed invocation's accepted obligations.
    /// Output custodies may still await their individually checked retirement.
    pub fn invocation_completed(&self) -> Option<bool> {
        self.resources
            .as_ref()
            .map(|resources| resources.lifetime.0.completed.load(Ordering::Acquire))
    }

    /// Provenance check only: the maintenance caller must independently
    /// prove its eligibility and charge the original budget and supervisor.
    fn resources_for(
        &mut self,
        registry: &XServerFrontendRouteRegistry,
        instance: u64,
    ) -> Result<&mut PrivateRetainedExecutionResources, PrivateExecutionBorrowRefusal> {
        let resources = self
            .resources
            .as_mut()
            .ok_or(PrivateExecutionBorrowRefusal::Unavailable)?;
        if resources.lifetime.0.instance != instance
            || !Arc::ptr_eq(&resources.origin.clients, &registry.clients)
        {
            return Err(PrivateExecutionBorrowRefusal::ForeignInvocation);
        }
        Ok(resources)
    }

    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "Inventory-specific borrowing remains a separate checked boundary"
        )
    )]
    fn resources_for_inventory(
        &mut self,
        inventory: &PrivateTerminalInventory,
    ) -> Result<&mut PrivateRetainedExecutionResources, PrivateExecutionBorrowRefusal> {
        let witness = inventory
            .execution
            .as_ref()
            .ok_or(PrivateExecutionBorrowRefusal::Unavailable)?;
        let resources = self.resources_for(&inventory.origin, witness.instance)?;
        if !Arc::ptr_eq(&resources.lifetime.0, witness) {
            return Err(PrivateExecutionBorrowRefusal::ForeignInvocation);
        }
        Ok(resources)
    }

    // Empty at service entry and exclusively borrowed throughout. No new
    // allocation, keyboard history, budget or supervisor is made at handoff.
    fn retain(
        &mut self,
        mut runner: PrivatePreparedRunner,
        collected: Option<PrivateConnectionsCollected>,
    ) -> PrivateXServerFrontend {
        assert!(
            self.resources.is_none(),
            "execution keeper already occupied"
        );
        runner.close_admission();
        let PrivatePreparedRunner {
            lifetime,
            watch,
            frontend,
            keyboards,
            namespace,
            seat,
            service_origin,
            service,
            prefer_cleanup,
            reclaim_cursor,
        } = runner;
        let frontend = frontend.expect("live runner until execution handoff");
        let origin = frontend.broker.registry.clone();
        let queue = Arc::clone(&frontend.admission.ready);
        let native_owner = frontend
            .native_owner
            .as_ref()
            .expect("prepared native origin")
            .clone();
        lifetime.0.state.store(1, Ordering::Release);
        self.resources = Some(PrivateRetainedExecutionResources {
            lifetime,
            watch,
            origin,
            native_owner,
            collected,
            queue,
            keyboards,
            namespace,
            seat,
            service_origin,
            service,
            prefer_cleanup,
            reclaim_cursor,
        });
        frontend
    }
}

#[cfg(unix)]
impl PrivateSettlement {
    /// Read execution availability without settling or changing any debt.
    pub fn execution(&self) -> Option<PrivateExecutionReading> {
        self.execution.as_ref().map(|witness| witness.reading())
    }
}

#[cfg(unix)]
impl PrivateSettlementOwner {
    /// Read the original execution availability beside each retained terminal
    /// inventory. None means the inventory could not be read, not no debt.
    pub fn retained_executions(&self) -> Option<Vec<PrivateExecutionReading>> {
        self.inner.lock().ok().map(|held| {
            held.terminal
                .iter()
                .filter_map(|inventory| {
                    inventory
                        .execution
                        .as_ref()
                        .map(|witness| witness.reading())
                })
                .chain(
                    held.terminal_in_flight
                        .iter()
                        .map(|witness| witness.reading()),
                )
                .collect()
        })
    }
}
