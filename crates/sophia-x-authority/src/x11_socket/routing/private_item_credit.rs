// The accepted ordered item's storage charge. Native holds and immutable
// output have their own pre-reserved storage after an established transfer.

#[cfg(unix)]
struct PrivateAcceptedItemCredit {
    store: std::sync::Weak<Mutex<AbandonedSettlements>>,
}

#[cfg(unix)]
impl PrivateReservation {
    /// Transfer the charge already reserved by SharedAdmission. Only the
    /// prepared dequeue calls this; unprepared routing keeps its existing
    /// identity-based settlement and must not own a second release right.
    fn accepted_in(self, store: &PrivateSettlementOwner) -> PrivateOutstandingRequest {
        let mut request = self.accepted();
        request.accepted_store_credit = Some(PrivateAcceptedItemCredit {
            store: Arc::downgrade(&store.inner),
        });
        request
    }
}

#[cfg(unix)]
impl PrivateOutstandingRequest {
    /// Dispose an item whose exact common completion was observed and whose
    /// event custody has transferred into separately reserved native/output
    /// storage. The caller removes the item immediately after this succeeds.
    /// Observation, a deadline and Drop alone never return storage credit.
    fn finish_item(&mut self) -> bool {
        if !self.observed.get() {
            return false;
        }
        if let Some(credit) = &self.accepted_store_credit {
            let Some(inner) = credit.store.upgrade() else {
                return false;
            };
            let store = PrivateSettlementOwner { inner };
            // Clear the sole release right before releasing. No callback or
            // fallible operation separates the two; poison recovery cannot
            // turn a completed disposal into a second capacity return.
            self.accepted_store_credit = None;
            store.release();
        }
        true
    }
}

#[cfg(unix)]
impl PrivateSettlementOwner {
    /// Immutable bound shared by every accepted operation, including old
    /// generations still retained after their grant slots were reused.
    fn accepted_item_capacity(&self) -> Result<usize, AdmissionRefusal> {
        self.inner
            .lock()
            .map(|held| held.capacity)
            .map_err(|_| AdmissionRefusal::Unavailable)
    }
}
