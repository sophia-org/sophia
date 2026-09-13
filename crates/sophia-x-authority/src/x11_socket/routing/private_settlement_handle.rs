// What an abandoning instance hands over, and how it discharges it.
//
// Split from the durable owner by subject: the owner is where obligations
// live once nothing holds them, and this is the handle that still does.

/// What a shutdown could not settle, and the means to settle it later.
///
/// Counting an unsettled obligation and logging it is a diagnostic, not a
/// transfer. Nor is handing back the work alone: a report holding only
/// operations would have thrown away the registry that could answer them, so
/// a caller would be left holding obligations and nothing to discharge them
/// with. This retains the originating capability along with the work.
///
/// The obligations themselves stay private. They carry the stamped envelope
/// shape, and exporting that so an out-of-crate owner could read a report
/// would be publishing the wire format to deliver a status. What a caller
/// needs is not to inspect them but to retry them, which it can.
#[cfg(unix)]
#[must_use = "unsettled work is owed an answer; retry or record the failure"]
pub struct PrivateSettlement {
    /// The capability that can answer the work, retained from the instance
    /// that accepted it. Not supplied by a caller: an external authority
    /// argument would let one instance's obligations be settled against
    /// another's registry.
    origin: XServerFrontendRouteRegistry,
    /// Where anything still owed goes if this handle is abandoned.
    durable: PrivateSettlementOwner,
    /// The queue this came from, retained so a failed instance hands over its
    /// queue rather than a note that one existed. The queue alone, not the
    /// admission that holds the owner: that would be a cycle.
    queue: Arc<Mutex<SharedQueue>>,
    pending: Vec<PrivateOperation>,
    /// Work that was routed and has not reached a terminal outcome.
    ///
    /// Carried from the instance rather than left to die with it. These hold
    /// credits, and some of them -- input with a tracked delivery -- can still
    /// finish, so destroying the identities would strand the credits and lose
    /// the only means of noticing.
    outstanding: Vec<PrivateIdentity>,
    queue_unreadable: bool,
    /// Which pending obligation an attempt is under way for.
    ///
    /// Written before the attempt can emit, so an unwind inside leaves it set
    /// and the next caller can tell that one obligation's outcome is unknown
    /// rather than retrying it. An index rather than a flag because this
    /// handle settles in place and leaves what it could not answer where it
    /// was, so position is what identifies the one being attempted.
    settling: Option<usize>,
}

#[cfg(unix)]
impl PrivateSettlement {
    pub fn is_settled(&self) -> bool {
        self.pending.is_empty() && self.outstanding.is_empty() && !self.queue_unreadable
    }

    /// How many obligations remain undischarged.
    pub fn owed(&self) -> usize {
        self.pending.len()
    }

    /// How many routed operations have not reached a terminal outcome.
    pub fn outstanding(&self) -> usize {
        self.outstanding.len()
    }

    /// How many control records the instance's registry still holds.
    ///
    /// Operations caught mid-application are the ones that stay: they are
    /// retained rather than reported as unexecuted, and they are reachable
    /// rather than counted and forgotten, because the registry holding them
    /// came with the origin this settlement kept.
    ///
    /// `None` where there is no registry to ask, or one that cannot be read.
    /// An instance with nothing outstanding and one nobody can look at are
    /// different answers.
    pub fn outstanding_control(&self) -> Option<usize> {
        self.origin
            .control_completion()
            .map(|owner| owner.outstanding())?
    }

    /// Republish acknowledgements a client writer could not deliver.
    ///
    /// Republishing only: the effects already happened, so nothing here is
    /// re-run. Returns how many reached the receiver.
    pub fn republish_owed_acknowledgements(&self) -> usize {
        let Some(owner) = self.origin.control_completion() else {
            return 0;
        };
        let sender = &self.origin.acknowledgement_sender;
        owner.publish_owed_with(|acknowledgement| match sender.try_send(*acknowledgement) {
            Ok(()) => ControlPublication::Delivered,
            Err(TrySendError::Disconnected(_)) => ControlPublication::ReceiverGone,
            Err(TrySendError::Full(_)) => ControlPublication::Retained,
        })
    }

    /// Release credits for carried work that has since finished.
    ///
    /// The same rule as on a live instance: ended releases, live and
    /// unreadable do not.
    pub fn reclaim_outstanding(&mut self) -> usize {
        // Applied here too, not only while the instance was live. A frontend
        // is consumed by shutting down, and a proof that only it could apply
        // would stop being applied exactly when the work outlives it.
        if let Some(owner) = self.origin.control_completion() {
            let _settled = owner.reconcile_unstarted();
        }
        let recovery = &self.origin.input_recovery;
        let recovery_origin = &self.origin;
        let before = self.outstanding.len();
        self.outstanding.retain(|identity| match identity {
            PrivateIdentity::Delivery(Some(delivery)) => {
                !matches!(recovery.delivery_state(*delivery), DeliveryState::Ended)
            }
            PrivateIdentity::Control {
                completion: Some(token),
                ..
            } => !matches!(
                recovery_origin
                    .control_completion()
                    .map(|owner| owner.state_of(*token)),
                Some(ControlRecordState::Retired)
            ),
            PrivateIdentity::Delivery(None)
            | PrivateIdentity::Control { completion: None, .. }
            | PrivateIdentity::Lease(_) => true,
        });
        let reclaimed = before.saturating_sub(self.outstanding.len());
        for _ in 0..reclaimed {
            self.durable.release();
        }
        reclaimed
    }

    /// Whether the queue could not be read, so its contents were unrecoverable.
    pub fn queue_unreadable(&self) -> bool {
        self.queue_unreadable
    }

    /// Try again to discharge what remains, against the origin that accepted
    /// it.
    ///
    /// Returns how many were discharged this time. Work that still cannot be
    /// answered stays pending rather than being counted off, so retrying twice
    /// does not answer anything twice.
    pub fn retry(&mut self) -> usize {
        self.park_interrupted();
        self.settle_pending()
    }

    /// Hand an interrupted attempt to the durable owner as unproved.
    ///
    /// Runs before anything else touches `pending`, so an attempt that never
    /// returned cannot be retried by the next caller. Whether its
    /// acknowledgement went out is exactly what the unwind destroyed, and this
    /// handle cannot find out: it is moved somewhere that keeps it and its
    /// credit without ever driving it again.
    fn park_interrupted(&mut self) {
        let Some(index) = self.settling.take() else {
            return;
        };
        if index >= self.pending.len() {
            return;
        }
        let unproved = self.pending.remove(index);
        self.durable.take_indeterminate(&self.origin, unproved);
    }

    /// One attempt at each pending obligation, in place.
    ///
    /// Nothing is moved out of `pending` to be settled. An obligation is
    /// removed once its attempt has returned and said what happened, so a
    /// fault before the attempt leaves it here and retryable, and a fault
    /// during the attempt leaves it here and marked. This handle can outlive
    /// either -- `retry` is called on a live one -- so losing the list to a
    /// stack frame would strand work whose owner is still in use.
    fn settle_pending(&mut self) -> usize {
        let mut answered = 0usize;
        let mut index = self.pending.len();
        while index > 0 {
            index -= 1;
            match ownership_of(&self.origin, &self.pending[index]) {
                SettlementOwnership::Elsewhere => {
                    // Answered by whoever holds the record. Carried on as an
                    // identity so its credit is released when that happens,
                    // never as a command that could be sent again.
                    let operation = self.pending.remove(index);
                    self.durable
                        .take_one_outstanding(&self.origin, PrivateIdentity::of(&operation));
                    continue;
                }
                // Kept, with its credit. Nothing here can show it is owed one
                // outcome rather than two.
                SettlementOwnership::Unprovable => continue,
                SettlementOwnership::Ours => {}
            }
            self.settling = Some(index);
            let settled = settle_one(&self.origin, &self.pending[index]);
            self.settling = None;
            if settled {
                self.pending.remove(index);
                self.durable.release();
                answered = answered.saturating_add(1);
            }
        }
        answered
    }
}

#[cfg(unix)]
impl Drop for PrivateSettlement {
    fn drop(&mut self) {
        self.park_interrupted();
        // Transferred whether or not anything else is owed. Returning early on
        // an empty pending list destroyed these, which is the abandonment loss
        // this handle exists to prevent, recreated in the state that was added
        // to prevent it.
        //
        // Handed over one at a time from the list this handle still owns. A
        // take into an argument empties the handle first, so a handover that
        // does not return leaves them in neither place.
        while let Some(identity) = self.outstanding.pop() {
            self.durable.take_one_outstanding(&self.origin, identity);
        }
        if self.queue_unreadable {
            // Owned by something that outlives this rather than surviving as a
            // boolean on a handle that is going away.
            self.durable
                .take_failed_instance(&self.origin, &self.queue);
        }
        if self.pending.is_empty() {
            return;
        }
        // One attempt, not a loop: a Drop that retried until it succeeded
        // would block teardown on a congested channel. What that attempt
        // cannot answer moves to the durable owner rather than being
        // destroyed here -- a full channel with a live receiver is congestion,
        // and removing the last owner is not evidence the obligation ended.
        let _answered = self.settle_pending();
        while let Some(operation) = self.pending.pop() {
            self.durable.take_one(&self.origin, operation);
        }
    }
}
