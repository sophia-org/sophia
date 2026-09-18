/// Established by the exact retained serving owner after actual frame
/// collection, join publication and fence. It is never a writer-flush receipt.
#[cfg(unix)]
struct PrivateRecipientTermination {
    endpoint: PrivateEndpointIdentity,
}

#[cfg(unix)]
impl PrivateRecipientTermination {
    fn from_place(
        service: &PrivateServiceLease<'_>,
        registry: &XServerFrontendRouteRegistry,
        collected: Option<&PrivateConnectionsCollected>,
        cursor: &mut usize,
        endpoint: &PrivateEndpointIdentity,
    ) -> Result<Self, PrivateTerminalDriveRefusal> {
        use PrivateTerminalDriveRefusal::RecipientUnavailable as Refused;
        let custody = {
            let kept = service.owner.inventory.kept.lock().map_err(|_| Refused)?;
            if kept.places.is_empty() {
                return Err(Refused);
            }
            let index = *cursor % kept.places.len();
            *cursor = (index + 1) % kept.places.len();
            kept.places[index].as_ref().cloned().ok_or(Refused)?
        };
        if !custody.cleanup_record().published_by(registry) {
            return Err(Refused);
        }
        custody
            .deferred_cleanup_prerequisites(collected)
            .map_err(|_| Refused)?;
        let fence = custody
            .fence_evidence()
            .fence()
            .filter(|fence| *fence != PrivateHandoverFence::Unreadable)
            .ok_or(Refused)?;
        let home = custody
            .cleanup_record()
            .ordered_home
            .state
            .lock()
            .map_err(|_| Refused)?;
        if home.standing != PrivateHomeStanding::Retained {
            return Err(Refused);
        }
        let Some(PrivateOrderedContinuation::Serving { owner, evidence }) = home.payload.as_ref()
        else {
            return Err(Refused);
        };
        if evidence.source_poisoned
            || evidence.fence != Some(fence)
            || !matches!(&evidence.worker, PrivateOrderedWorkerExit::Joined(join)
                if std::ptr::eq(join.as_ptr(), Arc::as_ptr(custody.join())))
            || !owner.served.endpoint().matches(endpoint)
            || !owner
                .closing()
                .is_some_and(|close| close.termination == X11OrderedTermination::Established)
        {
            return Err(Refused);
        }
        Ok(Self {
            endpoint: owner.served.endpoint().clone(),
        })
    }
}

#[cfg(unix)]
impl PrivateTerminalInventory {
    fn retire_native_one(
        &mut self,
        service: &PrivateServiceLease<'_>,
        collected: Option<&PrivateConnectionsCollected>,
        cursor: &mut PrivateTerminalDriveCursor,
    ) -> Result<PrivateTerminalVisit, PrivateTerminalDriveRefusal> {
        use PrivateTerminalDriveRefusal as Refusal;
        if !self.native_disposal_ready(cursor) {
            return Ok(PrivateTerminalVisit::Disposed { records: 0 });
        }
        let count = self.holds.len() + self.settling.len() + 1;
        let index = cursor.recipient % count;
        let (incarnation, grant, endpoint, proof, attempt) = if index < self.holds.len() {
            let hold = &self.holds[index];
            let native = hold.native.as_ref().ok_or(Refusal::MissingNative)?;
            (
                hold.incarnation,
                native.grant(),
                native.endpoint(),
                native.proof(),
                hold.custody.attempt,
            )
        } else if index < count - 1 {
            let release = &self.settling[index - self.holds.len()];
            let native = release.native.as_ref().ok_or(Refusal::MissingNative)?;
            (
                release.incarnation,
                native.grant(),
                native.endpoint(),
                native.proof(),
                release.custody.attempt,
            )
        } else {
            match &self.native_pending {
                PrivateNativePending::Pointer(Some(hold)) => (
                    hold.incarnation().ok_or(Refusal::MissingIncarnation)?,
                    hold.grant(),
                    hold.endpoint(),
                    hold.proof(),
                    self.pending_custody
                        .as_ref()
                        .and_then(|custody| custody.attempt),
                ),
                PrivateNativePending::Key(Some(hold)) => (
                    hold.incarnation().ok_or(Refusal::MissingIncarnation)?,
                    hold.grant(),
                    hold.endpoint(),
                    hold.proof(),
                    self.pending_custody
                        .as_ref()
                        .and_then(|custody| custody.attempt),
                ),
                _ => {
                    cursor.next_recipient((index + 1) % count);
                    return Ok(PrivateTerminalVisit::Disposed { records: 0 });
                }
            }
        };
        let proof = proof.ok_or(Refusal::MissingNative)?;
        if proof.incarnation() != incarnation {
            return Err(Refusal::MissingIncarnation);
        }
        let ended = PrivateRecipientTermination::from_place(
            service,
            &self.origin,
            collected,
            &mut cursor.custody,
            endpoint,
        );
        // Traverse the product of records and physical custody places. Two
        // independently advanced cursors could miss every matching pair.
        if cursor.custody == 0 {
            cursor.next_recipient((index + 1) % count);
        }
        let ended = ended?;
        if !ended.endpoint.matches(endpoint) {
            return Err(Refusal::RecipientUnavailable);
        }
        // Record the source-owned native bit before answering the recipient.
        // Neither a retained record nor an ended wire supplies native proof.
        proof.record_native().map_err(Refusal::Common)?;
        let settled = self
            .controller
            .under_common_as_origin(|authority, issuer| {
                if let Some(attempt) = attempt {
                    authority.finish_attempt(
                        issuer,
                        attempt,
                        sophia_input_authority::SettlementBit {
                            native_reconciled: false,
                            recipient_settled: true,
                        },
                    )?;
                }
                authority.settle(
                    issuer,
                    Some(grant),
                    incarnation.input,
                    incarnation,
                    sophia_input_authority::SettlementBit {
                        native_reconciled: false,
                        recipient_settled: true,
                    },
                )?;
                authority
                    .reconciliation_record_present(issuer, incarnation)
                    .map(|present| !present)
            })
            .map_err(Refusal::Common)?
            .map_err(|cause| Refusal::Common(PrivateAuthorityRefusal::Authority(cause)))?;
        if !settled {
            return Ok(PrivateTerminalVisit::Recipient { settled: false });
        }
        if self
            .attempt_custody
            .is_some_and(|held| Some(held.token) == attempt)
        {
            self.attempt_custody = None;
        }
        // Both independent source facts and the common ledger's completed
        // state precede destruction of the exact source, events and custody.
        if index < self.holds.len() {
            self.holds.remove(index);
        } else if index < count - 1 {
            self.settling.remove(index - self.holds.len());
        } else {
            let _ = self.native_pending.take();
            self.pending_custody = None;
        }
        cursor.next_recipient(index);
        self.shared_activation.invalidate();
        Ok(PrivateTerminalVisit::Disposed { records: 1 })
    }
}
