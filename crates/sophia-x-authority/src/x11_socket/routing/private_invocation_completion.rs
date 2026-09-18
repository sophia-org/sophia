/// One row of one obligation class is examined per charged visit. Any move
/// between the store's classes invalidates the scan before it can certify.
#[cfg(unix)]
#[derive(Default)]
struct PrivateInvocationCompletionCursor {
    epoch: Option<u64>,
    class: u8,
    row: usize,
    custody: usize,
}

#[cfg(unix)]
impl AbandonedSettlements {
    fn obligations_changed(&mut self) {
        self.obligation_epoch = self.obligation_epoch.and_then(|epoch| epoch.checked_add(1));
    }
}

#[cfg(unix)]
impl PrivateInvocationCompletionCursor {
    fn restart(&mut self, epoch: Option<u64>) {
        self.epoch = epoch;
        self.class = 0;
        self.row = 0;
    }

    fn observe(&mut self, matching: Option<bool>) -> PrivateTerminalVisit {
        match matching {
            Some(true) => {
                self.restart(self.epoch);
                PrivateTerminalVisit::InvocationOutstanding
            }
            Some(false) => {
                self.row += 1;
                PrivateTerminalVisit::InvocationScanning
            }
            None => {
                self.class += 1;
                self.row = 0;
                PrivateTerminalVisit::InvocationScanning
            }
        }
    }
}

#[cfg(unix)]
impl PrivateRetainedExecutionResources {
    fn visit_invocation_completion(
        witness: &Arc<PrivateExecutionWitness>,
        origin: &XServerFrontendRouteRegistry,
        queue: &Arc<Mutex<SharedQueue>>,
        collected: Option<&PrivateConnectionsCollected>,
        service: &PrivateServiceLease<'_>,
        cursor: &mut PrivateInvocationCompletionCursor,
    ) -> Result<PrivateTerminalVisit, PrivateTerminalDriveRefusal> {
        use PrivateTerminalDriveRefusal as Refusal;
        if !witness.handed_off.load(Ordering::Acquire) {
            cursor.restart(None);
            return Ok(PrivateTerminalVisit::SettlementStillOwned);
        }
        if witness.completed.load(Ordering::Acquire) {
            return Self::retire_completed_custody(witness, origin, collected, service, cursor);
        }
        if cursor.class == 13 {
            // A closed-empty queue is a positive read of the original queue,
            // taken without the aggregate store held. The final epoch check
            // then excludes movements throughout every row of this scan.
            let empty = {
                let queue = queue.lock().map_err(|_| Refusal::StoreUnreadable)?;
                queue.closed && queue.ready.is_empty()
            };
            let store = service
                .store()
                .inner
                .lock()
                .map_err(|_| Refusal::StoreUnreadable)?;
            let stable = cursor.epoch.is_some() && store.obligation_epoch == cursor.epoch;
            if !empty || !stable {
                cursor.restart(store.obligation_epoch);
                return Ok(PrivateTerminalVisit::InvocationOutstanding);
            }
            witness.completed.store(true, Ordering::Release);
            return Ok(PrivateTerminalVisit::InvocationCompleted);
        }
        let store = service
            .store()
            .inner
            .lock()
            .map_err(|_| Refusal::StoreUnreadable)?;
        let epoch = store
            .obligation_epoch
            .ok_or(Refusal::CompletionEpochExhausted)?;
        if cursor.epoch != Some(epoch) {
            cursor.restart(Some(epoch));
        }
        if cursor.class == 12 {
            drop(store);
            let custody = {
                let kept = service
                    .owner
                    .inventory
                    .kept
                    .lock()
                    .map_err(|_| Refusal::StoreUnreadable)?;
                kept.places.get(cursor.row).cloned()
            };
            let Some(custody) = custody else {
                return Ok(cursor.observe(None));
            };
            if let Some(custody) = custody
                && custody.cleanup_record().published_by(origin)
            {
                Self::completed_custody_evidence(&custody, collected)?;
            }
            return Ok(cursor.observe(Some(false)));
        }
        let ours =
            |other: &XServerFrontendRouteRegistry| Arc::ptr_eq(&origin.clients, &other.clients);
        let row = cursor.row;
        let matching = match cursor.class {
            0 => store.held.get(row).map(|(origin, _)| ours(origin)),
            1 => store.in_flight.get(row).map(|(origin, _)| ours(origin)),
            2 => store.outstanding.get(row).map(|(origin, _)| ours(origin)),
            3 => store
                .outstanding_in_flight
                .get(row)
                .map(|(origin, _)| ours(origin)),
            4 => store.indeterminate.get(row).map(|(origin, _)| ours(origin)),
            5 => store.failed.get(row).map(|failed| ours(&failed.origin)),
            6 => store
                .failed_in_flight
                .get(row)
                .map(|failed| ours(&failed.origin)),
            7 => store
                .terminal
                .get(row)
                .map(|terminal| ours(&terminal.origin)),
            8 => store
                .terminal_in_flight
                .get(row)
                .map(|other| Arc::ptr_eq(witness, other)),
            9 => store
                .unresolved_egress
                .get(row)
                .map(|(instance, _)| *instance == witness.instance),
            10 => store.continuations.get(row).map(|place| match place {
                PrivateOrderedContinuationPlace::Free => false,
                PrivateOrderedContinuationPlace::Taken(home) => home.may_belong_to(origin),
            }),
            _ => store.holders.get(row).map(|place| match place {
                PrivateHolderPlace::Free => false,
                PrivateHolderPlace::Taken(holder) => holder
                    .credit
                    .maintenance_identity()
                    .home
                    .upgrade()
                    .is_none_or(|home| home.may_belong_to(origin)),
                // An unfinished destination does not identify a discharged
                // obligation. Retain it until its original source answers.
                _ => true,
            }),
        };
        Ok(cursor.observe(matching))
    }

    /// Release only the exact original published custody whose source output
    /// is settled and whose original slot/holder/destination were returned.
    /// Native receipt dependents are excluded by positive invocation completion.
    fn retire_completed_custody(
        witness: &Arc<PrivateExecutionWitness>,
        origin: &XServerFrontendRouteRegistry,
        collected: Option<&PrivateConnectionsCollected>,
        service: &PrivateServiceLease<'_>,
        cursor: &mut PrivateInvocationCompletionCursor,
    ) -> Result<PrivateTerminalVisit, PrivateTerminalDriveRefusal> {
        use PrivateTerminalDriveRefusal as Refusal;
        if !witness.completed.load(Ordering::Acquire) {
            return Ok(PrivateTerminalVisit::InvocationOutstanding);
        }
        // A later unreadable aggregate cannot authorize releasing its custody.
        drop(
            service
                .store()
                .inner
                .lock()
                .map_err(|_| Refusal::StoreUnreadable)?,
        );
        let (index, custody) = {
            let kept = service
                .owner
                .inventory
                .kept
                .lock()
                .map_err(|_| Refusal::StoreUnreadable)?;
            if kept.places.is_empty() {
                return Ok(PrivateTerminalVisit::CustodyRetired { retired: false });
            }
            let index = cursor.custody % kept.places.len();
            cursor.custody = (index + 1) % kept.places.len();
            (index, kept.places[index].as_ref().cloned())
        };
        let Some(custody) = custody else {
            return Ok(PrivateTerminalVisit::CustodyRetired { retired: false });
        };
        if !custody.cleanup_record().published_by(origin) {
            return Ok(PrivateTerminalVisit::OtherInvocation);
        }
        Self::completed_custody_evidence(&custody, collected)?;
        let removed = {
            let mut kept = service
                .owner
                .inventory
                .kept
                .lock()
                .map_err(|_| Refusal::StoreUnreadable)?;
            if !kept
                .places
                .get(index)
                .and_then(Option::as_ref)
                .is_some_and(|other| Arc::ptr_eq(other, &custody))
            {
                return Err(Refusal::RecipientUnavailable);
            }
            let removed = kept.places[index].take();
            kept.taken -= 1;
            removed
        };
        // No aggregate guard is held while source payloads or join evidence
        // lose their last owner. Reader-held Arc pins may intentionally remain.
        drop(removed);
        Ok(PrivateTerminalVisit::CustodyRetired { retired: true })
    }

    fn completed_custody_evidence(
        custody: &PrivateEvidenceCustody,
        collected: Option<&PrivateConnectionsCollected>,
    ) -> Result<(), PrivateTerminalDriveRefusal> {
        use PrivateTerminalDriveRefusal as Refusal;
        custody
            .deferred_cleanup_prerequisites(collected)
            .map_err(|_| Refusal::RecipientUnavailable)?;
        if !matches!(custody.join().result(), Some(PrivateJoinResult::Returned)) {
            // Original opaque failure evidence has no disposal acknowledgement
            // on this path, even if its native and transport work is finished.
            return Err(Refusal::WorkerFailureRetained);
        }
        let fence = custody
            .fence_evidence()
            .fence()
            .filter(|fence| *fence != PrivateHandoverFence::Unreadable)
            .ok_or(Refusal::RecipientUnavailable)?;
        {
            let cleanup = custody
                .source
                .deferred_cleanup
                .lock()
                .map_err(|_| Refusal::StoreUnreadable)?;
            if !matches!(*cleanup, PrivateDeferredCleanupStanding::Done(report)
                if report.committed && report.closure == fence
                    && report.namespace == PrivateNamespaceClearance::Established)
            {
                return Err(Refusal::RecipientUnavailable);
            }
        }
        let home = &custody.cleanup_record().ordered_home;
        if !std::ptr::eq(custody.identity().home.as_ptr(), Arc::as_ptr(home))
            || !home.storage_returned.load(Ordering::Acquire)
        {
            return Err(Refusal::RecipientUnavailable);
        }
        {
            let held = home.state.lock().map_err(|_| Refusal::StoreUnreadable)?;
            if held.standing != PrivateHomeStanding::Retained {
                return Err(Refusal::RecipientUnavailable);
            }
            let continuation = held.payload.as_ref().ok_or(Refusal::RecipientUnavailable)?;
            let evidence = match continuation {
                PrivateOrderedContinuation::Setup { evidence, .. }
                | PrivateOrderedContinuation::Serving { evidence, .. } => evidence,
            };
            if evidence.source_poisoned
                || evidence.fence != Some(fence)
                || !matches!(&evidence.worker, PrivateOrderedWorkerExit::Joined(join)
                    if std::ptr::eq(join.as_ptr(), Arc::as_ptr(custody.join())))
                || !continuation.settled()
            {
                return Err(Refusal::RecipientUnavailable);
            }
        }
        Ok(())
    }
}
