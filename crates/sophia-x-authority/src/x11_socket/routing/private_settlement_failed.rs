// The failed-instance retention of the settlement store: an instance that
// could not finish is kept whole -- origin, queue, the reason it stands and
// the failure slot it still charges -- and recovered only when nothing
// stands against it.
//
// Split by subject from the store: the owner's live accounting stays there;
// what it keeps of instances that failed, and the one recovery over them,
// is here.

/// One instance that closed with a queue nobody could read.
#[cfg(unix)]
struct FailedInstance {
    origin: XServerFrontendRouteRegistry,
    queue: Arc<Mutex<SharedQueue>>,
    /// Places whose registered worker was never collected when this
    /// instance was retained.
    ///
    /// THE REASON TRAVELS WITH THE RETENTION. An instance kept because an
    /// actor is still uncollected is not an ordinary recoverable failure:
    /// recovery leaves it standing, drains nothing from it and releases no
    /// charge for it, until something authorised to collect the actor does
    /// -- which nothing here is.
    uncollected: Vec<usize>,
    /// Whether this instance's failure slot has been given back.
    ///
    /// Carried on the record rather than inferred from the record being gone.
    /// A slot is released for one failure, and the only thing that identifies
    /// that failure is this record, so the fact that its slot was released has
    /// to live here: recovery can be interrupted after releasing and before
    /// removing it, and a restored record with no such state is
    /// indistinguishable from one that never released. Releasing again then
    /// hands back a slot this failure does not hold, which is another live
    /// instance's, and the count admits one more instance than the bound
    /// allows.
    slot: FailureSlot,
}

/// Whether a failed instance still holds the slot it reserved.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FailureSlot {
    /// Reserved before the instance was exposed and not yet given back.
    Held,
    /// Given back. Marked before the count is changed, so an interruption in
    /// between under-releases -- costing this owner one slot for its life --
    /// rather than releasing twice. One direction loses capacity, the other
    /// hands out capacity that does not exist.
    Released,
}

#[cfg(unix)]
impl PrivateSettlementOwner {
    /// How many instances closed holding a queue nobody could read.
    ///
    /// Each is retained with its queue and its registry, so it can be examined
    /// rather than merely counted.
    /// `None` where the owner cannot be read.
    pub fn failed_instances(&self) -> Option<usize> {
        self.inner.lock().ok().map(|held| held.failed.len())
    }

    /// How many instance failure slots are charged right now, live and
    /// retained together.
    pub fn failure_slots_charged(&self) -> Option<usize> {
        self.inner.lock().ok().map(|held| held.failure_slots)
    }

    fn take_failed_instance(
        &self,
        origin: &XServerFrontendRouteRegistry,
        queue: &Arc<Mutex<SharedQueue>>,
        uncollected: Vec<usize>,
    ) {
        let mut held = self.records_even_if_poisoned();
        {
            // No capacity check. This instance reserved its slot before it was
            // exposed, so the space is already its own; refusing here would be
            // refusing after the failure, with nowhere to put what is refused.
            held.failed.push(FailedInstance {
                origin: origin.clone(),
                queue: Arc::clone(queue),
                uncollected,
                slot: FailureSlot::Held,
            });
        }
    }

    /// The places of every retained instance whose registered worker is
    /// still uncollected, one entry per instance.
    ///
    /// THE STANDING REASON, READABLE. These instances are kept out of
    /// recovery; what a caller may do about the actor is not decided here.
    pub fn uncollected_instances(&self) -> Option<Vec<Vec<usize>>> {
        self.inner.lock().ok().map(|held| {
            held.failed
                .iter()
                .chain(held.failed_in_flight.iter())
                .filter(|instance| !instance.uncollected.is_empty())
                .map(|instance| instance.uncollected.clone())
                .collect()
        })
    }

    /// Recover what a failed instance's queue still holds, and answer it.
    ///
    /// A poisoned lock stays poisoned, but the data behind it is intact, so
    /// the obligations are readable even though the instance that accepted
    /// them is not usable. Nothing is resumed: what comes out is settled
    /// against the registry that accepted it, exactly as abandoned work is.
    /// This is what retaining the queue was for -- a tally could have been
    /// counted but never discharged.
    /// `None` where the owner cannot be read: recovering nothing and being
    /// unable to try are different answers, and only one of them says a later
    /// attempt might do something.
    pub fn recover_failed(&self) -> Option<usize> {
        let Ok(mut held) = self.inner.lock() else {
            return None;
        };
        // Moved into the owner's own in-flight list rather than a local, and
        // taken one at a time, so an unwind part-way through leaves the rest
        // here instead of dropping them with the frame. Appended rather than
        // taken, so the buffer reserved at construction survives and the next
        // failure does not allocate during cleanup -- exactly what reserving
        // it was meant to avoid.
        {
            let AbandonedSettlements {
                failed,
                failed_in_flight,
                ..
            } = &mut *held;
            // AN INSTANCE RETAINED OVER AN UNCOLLECTED ACTOR STANDS. It is not
            // moved into recovery, nothing is drained from its queue and its
            // charge is not released: recovering it would be settling over
            // the actor by another name.
            let (standing, recoverable): (Vec<FailedInstance>, Vec<FailedInstance>) =
                std::mem::take(failed)
                    .into_iter()
                    .partition(|instance| !instance.uncollected.is_empty());
            *failed = standing;
            failed_in_flight.extend(recoverable);
        }
        let mut recovered = 0usize;
        while !held.failed_in_flight.is_empty() {
            {
                // Out of the failed queue and into this owner's in-flight list
                // directly. Collecting them into a local first is the widest
                // window of the three sweeps: the queue no longer has them and
                // nothing else does either, so an unwind loses a whole
                // instance's worth of accepted work at once. Each operation
                // keeps the origin it was accepted against, so what answers it
                // is still the registry that took it.
                let AbandonedSettlements {
                    failed_in_flight,
                    in_flight,
                    ..
                } = &mut *held;
                let instance = failed_in_flight.last().expect("not empty");
                let mut queue = match instance.queue.lock() {
                    Ok(queue) => queue,
                    // The guard is recoverable even though the lock is not: the
                    // work is still there and is still owed an answer.
                    Err(poisoned) => poisoned.into_inner(),
                };
                while let Some((_, _, operation)) = queue.ready.take_next() {
                    in_flight.push((instance.origin.clone(), operation));
                }
            }
            // Drained, so this instance's failure is resolved and its slot is
            // free for another. Marked on the record before the count moves,
            // and only if this record still holds it: an emptied record that
            // is restored and recovered a second time must not hand back a
            // slot another live instance is holding.
            let releasing = {
                let instance = held.failed_in_flight.last_mut().expect("not empty");
                let releasing = instance.slot == FailureSlot::Held;
                instance.slot = FailureSlot::Released;
                releasing
            };
            if releasing {
                held.failure_slots = held.failure_slots.saturating_sub(1);
            }
            let _emptied = held.failed_in_flight.pop().expect("not empty");
            recovered = recovered.saturating_add(held.sweep_in_flight());
        }
        Some(recovered)
    }
}
