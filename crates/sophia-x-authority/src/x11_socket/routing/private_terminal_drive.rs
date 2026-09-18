// One charged visit to the exact terminal inventory whose execution history
// the same-thread keeper retained. No production request is reconstructed.

#[cfg(unix)]
#[derive(Default)]
struct PrivateTerminalDriveCursor {
    completion: PrivateInvocationCompletionCursor,
    inventory: usize,
    native: usize,
    recording: usize,
    recipient: usize,
    custody: usize,
    requests: usize,
    phase: u8,
    disposal_scan: usize,
    disposal_missing: bool,
    disposal_ready: bool,
}

#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrivateTerminalDriveRefusal {
    CompletionEpochExhausted,
    WorkerFailureRetained,
    ExecutionNotRetained,
    ForeignServiceOwner,
    Uncollected,
    StoreUnreadable,
    InventoryBusy,
    Lifecycle(PrivateLifecycleRefusal),
    Native(private_native::Refusal),
    Common(PrivateAuthorityRefusal),
    MissingNative,
    MissingIncarnation,
    RecipientUnavailable,
    SupervisorMissing,
    Supervisor(private_watchdog::PrivateWatchdogRefusal),
}

#[cfg(unix)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum PrivateTerminalVisit {
    SettlementStillOwned,
    InvocationScanning,
    InvocationOutstanding,
    InvocationCompleted,
    CustodyRetired { retired: bool },
    EmptyInventory,
    OtherInvocation,
    Lifecycle { completed: usize },
    Native { reconciled: bool },
    SharedActivation { observed: usize, joined: usize },
    Recorded { settled: bool },
    Recipient { settled: bool },
    Disposed { records: usize },
    Request { disposed: bool },
    Transient { disposed: bool },
}

#[cfg(unix)]
impl std::fmt::Debug for PrivateTerminalVisit {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SettlementStillOwned => formatter.write_str("SettlementStillOwned"),
            Self::InvocationScanning => formatter.write_str("InvocationScanning"),
            Self::InvocationOutstanding => formatter.write_str("InvocationOutstanding"),
            Self::InvocationCompleted => formatter.write_str("InvocationCompleted"),
            Self::CustodyRetired { retired } => formatter
                .debug_struct("CustodyRetired")
                .field("retired", retired)
                .finish(),
            Self::Request { disposed } => formatter
                .debug_struct("Request")
                .field("disposed", disposed)
                .finish(),
            Self::EmptyInventory => formatter.write_str("EmptyInventory"),
            Self::OtherInvocation => formatter.write_str("OtherInvocation"),
            Self::Lifecycle { completed } => formatter
                .debug_struct("Lifecycle")
                .field("completed", completed)
                .finish(),
            Self::Native { reconciled } => formatter
                .debug_struct("Native")
                .field("reconciled", reconciled)
                .finish(),
            Self::SharedActivation { observed, joined } => formatter
                .debug_struct("SharedActivation")
                .field("observed", observed)
                .field("joined", joined)
                .finish(),
            Self::Recorded { settled } => formatter
                .debug_struct("Recorded")
                .field("settled", settled)
                .finish(),
            Self::Recipient { settled } => formatter
                .debug_struct("Recipient")
                .field("settled", settled)
                .finish(),
            Self::Disposed { records } => formatter
                .debug_struct("Disposed")
                .field("records", records)
                .finish(),
            Self::Transient { disposed } => formatter
                .debug_struct("Transient")
                .field("disposed", disposed)
                .finish(),
        }
    }
}

#[cfg(unix)]
#[derive(Debug)]
enum PrivateTerminalDriveStep {
    Refused(PrivateTerminalDriveRefusal),
    Yield(sophia_input_authority::ServiceStartRefusal),
    Charged {
        outcome: Result<PrivateTerminalVisit, PrivateTerminalDriveRefusal>,
        charge: Result<
            sophia_input_authority::ServiceCharge,
            sophia_input_authority::ServiceAccountingError,
        >,
        supervision: Result<(), private_watchdog::PrivateWatchdogRefusal>,
    },
}

/// The store remains the durable owner while the exact original executor
/// borrows an inventory. A visible marker and an unwind guard are installed
/// before releasing the aggregate. Neither native nor common runs under it.
#[cfg(unix)]
struct PrivateTerminalBorrow {
    store: PrivateSettlementOwner,
    witness: Arc<PrivateExecutionWitness>,
    inventory: Option<PrivateTerminalInventory>,
}

#[cfg(unix)]
impl Drop for PrivateTerminalBorrow {
    fn drop(&mut self) {
        let inventory = self
            .inventory
            .take()
            .filter(|inventory| !inventory.is_empty());
        // No effects while recovering a poisoned aggregate: this only returns
        // the exact accepted obligation into its pre-reserved storage.
        let mut held = self.store.records_even_if_poisoned();
        if let Some(inventory) = inventory {
            held.terminal.push(inventory);
        }
        if let Some(index) = held
            .terminal_in_flight
            .iter()
            .position(|witness| Arc::ptr_eq(witness, &self.witness))
        {
            held.terminal_in_flight.swap_remove(index);
        }
    }
}

#[cfg(unix)]
impl PrivateRetainedExecutionResources {
    fn drive_terminal_step(
        &mut self,
        service_owner: &PrivateServiceLease<'_>,
        cursor: &mut PrivateTerminalDriveCursor,
    ) -> PrivateTerminalDriveStep {
        use PrivateTerminalDriveRefusal as Refusal;
        use sophia_input_authority::{CleanupReadiness, ServiceWork};
        if self.lifetime.0.reading().availability != PrivateExecutionAvailability::Retained {
            return PrivateTerminalDriveStep::Refused(Refusal::ExecutionNotRetained);
        }
        if !self.origin.leased_by(service_owner) {
            return PrivateTerminalDriveStep::Refused(Refusal::ForeignServiceOwner);
        }
        if !self
            .collected
            .as_ref()
            .is_some_and(|token| Arc::ptr_eq(&token.registry, &self.origin.clients))
        {
            return PrivateTerminalDriveStep::Refused(Refusal::Uncollected);
        }
        let Some(watch) = self.watch.as_ref() else {
            return PrivateTerminalDriveStep::Refused(Refusal::SupervisorMissing);
        };
        let admission = match self.service.prepare(
            self.service_origin.elapsed(),
            ServiceWork::Cleanup,
            CleanupReadiness::Eligible,
        ) {
            Ok(admission) => admission,
            Err(cause) => return PrivateTerminalDriveStep::Yield(cause),
        };
        let began = std::time::Instant::now();
        let Some(elapsed) = began.checked_duration_since(self.service_origin) else {
            return PrivateTerminalDriveStep::Yield(
                sophia_input_authority::ServiceStartRefusal::ClockRegressed,
            );
        };
        let run = match admission.dequeued(elapsed, CleanupReadiness::Eligible) {
            Ok(run) => run,
            Err(cause) => return PrivateTerminalDriveStep::Yield(cause),
        };
        let (outcome, supervision) = match watch.begin_dequeued(began) {
            Err(cause) => (Err(Refusal::Supervisor(cause)), Err(cause)),
            Ok(mut watched) => match watched.applying() {
                Err(cause) => (Err(Refusal::Supervisor(cause)), Err(cause)),
                Ok(()) => {
                    let phase = cursor.phase;
                    cursor.phase = (phase + 1) % 8;
                    let outcome = if phase == 7 {
                        Self::visit_invocation_completion(
                            &self.lifetime.0,
                            &self.origin,
                            &self.queue,
                            self.collected.as_ref(),
                            service_owner,
                            &mut cursor.completion,
                        )
                    } else {
                        Self::visit_terminal(
                            &self.lifetime.0,
                            &self.origin,
                            &self.native_owner,
                            &mut self.keyboards,
                            self.collected.as_ref(),
                            service_owner,
                            cursor,
                            phase,
                        )
                    };
                    let supervision = watched.finish();
                    (outcome, supervision)
                }
            },
        };
        let charge = run.finish(self.service_origin.elapsed());
        PrivateTerminalDriveStep::Charged {
            outcome,
            charge,
            supervision,
        }
    }

    fn visit_terminal(
        witness: &Arc<PrivateExecutionWitness>,
        origin: &XServerFrontendRouteRegistry,
        native_owner: &private_native::Owner,
        keyboards: &mut PrivateKeyboards,
        collected: Option<&PrivateConnectionsCollected>,
        service_owner: &PrivateServiceLease<'_>,
        cursor: &mut PrivateTerminalDriveCursor,
        phase: u8,
    ) -> Result<PrivateTerminalVisit, PrivateTerminalDriveRefusal> {
        use PrivateTerminalDriveRefusal as Refusal;
        let mut visit = {
            let store = service_owner.store();
            let mut held = store.inner.lock().map_err(|_| Refusal::StoreUnreadable)?;
            if held
                .terminal_in_flight
                .iter()
                .any(|other| Arc::ptr_eq(witness, other))
            {
                return Err(Refusal::InventoryBusy);
            }
            if held.terminal.is_empty() {
                return Ok(PrivateTerminalVisit::EmptyInventory);
            }
            let index = cursor.inventory % held.terminal.len();
            cursor.inventory = (index + 1) % held.terminal.len();
            let inventory = &held.terminal[index];
            if !inventory
                .execution
                .as_ref()
                .is_some_and(|other| Arc::ptr_eq(witness, other))
                || !Arc::ptr_eq(&origin.clients, &inventory.origin.clients)
            {
                return Ok(PrivateTerminalVisit::OtherInvocation);
            }
            // Space is reserved before the original service can admit work.
            assert!(held.terminal_in_flight.len() < held.terminal_in_flight.capacity());
            held.obligations_changed();
            held.terminal_in_flight.push(witness.clone());
            PrivateTerminalBorrow {
                store: store.clone(),
                witness: witness.clone(),
                inventory: Some(held.terminal.swap_remove(index)),
            }
        };
        let inventory = visit
            .inventory
            .as_mut()
            .expect("the unwind guard owns the inventory");
        match phase {
            0 => inventory
                .lifecycle
                .drive(NonZeroUsize::new(1).unwrap())
                .map(|completed| PrivateTerminalVisit::Lifecycle { completed })
                .map_err(Refusal::Lifecycle),
            1 => inventory.reconcile_native_one(native_owner, keyboards, &mut cursor.native),
            2 => {
                let (observed, joined) = inventory
                    .shared_activation
                    .visit(&mut inventory.settling)
                    .unwrap_or((0, 0));
                Ok(PrivateTerminalVisit::SharedActivation { observed, joined })
            }
            3 => inventory.record_terminal_native_one(&mut cursor.recording),
            4 => inventory.retire_native_one(service_owner, collected, cursor),
            5 => inventory.retire_request_one(&mut cursor.requests),
            _ => Ok(PrivateTerminalVisit::Transient {
                disposed: inventory.transients.observe_one().unwrap_or(false),
            }),
        }
    }
}
