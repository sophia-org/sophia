//! What the controller takes off the service's queues, and what it holds.
//!
//! SPLIT FROM THE HANDLE BY SUBJECT, not to move lines. Standing a service up
//! and keeping what it owes are different jobs; the receipt ledger, its bounds
//! and every drain that feeds it belong together, and they are what pushed the
//! handle past the size the layout gate allows.
//!
//! A sibling module rather than a child, deliberately: `pub(super)` here means
//! the same thing it means in the handle, so nothing about who may call what
//! changes with the move.

use std::time::Duration;

use sophia_x_authority::{
    XAuthorityClientControlAck, XAuthorityClientInputDelivery, XAuthorityObservedTransactionBatch,
};

use super::handle::{PrivateInputHandle, PrivateInputUnavailable};

/// The most one drain takes at once.
///
/// BOUNDED, AND THE TAIL STAYS QUEUED. A live producer can fill a channel as
/// fast as a reader empties it, so draining until empty is a loop with no
/// promise of ending. Whatever is left stays in its own channel for the next
/// call rather than being dropped.
pub const PRIVATE_INPUT_DRAIN_BOUND: usize = 256;

/// The most receipts this service will hold in its own custody.
///
/// SEPARATE FROM THE PER-CALL BOUND. One bounds how much work a single call
/// does; this bounds how much a service can be holding at once. Reaching it
/// leaves the remainder in the channel, which is a queue already.
pub const PRIVATE_INPUT_RECEIPT_RETENTION: usize = 256;

/// Receipts taken from the delivery channel, and what became of each.
///
/// TWO DIFFERENT OUTCOMES, KEPT APART. An observed receipt released its
/// delivery's place in the ledger. A retained one did not, is still held, and
/// will be offered again; it is owed work rather than a receipt that was dealt
/// with, and one list holding both would make those the same answer.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PrivateInputReceipts {
    /// Handed back to the ledger, which pruned the ticket.
    pub observed: Vec<XAuthorityClientInputDelivery>,
    /// How many receipts this service is still holding, across every call.
    ///
    /// INVENTORY, NOT THIS CALL'S WORK, AND A COUNT RATHER THAN A LIST. The
    /// observations above are bounded; this is not, so returning the receipts
    /// themselves would make an unbounded copy of everything held on every
    /// call, and would blur the one distinction that matters here -- what this
    /// call did against what the service owes.
    pub retained: usize,
    /// How many receipts this call actually looked at.
    ///
    /// THE BOUND IS ON THIS, AND IT HAS TO BE VISIBLE. An earlier version
    /// bounded the work by what was left at the end of the call, which let a
    /// full retention queue be observed and a full channel drained on top of
    /// it in one call -- twice the advertised bound, and invisible from the
    /// outside. Reporting what was visited is what makes the bound checkable.
    pub visited: usize,
    /// How many observations could not be made because the ledger could not be
    /// read.
    ///
    /// COUNTED APART FROM THE REST OF `retained`. A ledger that was never asked
    /// has refused nothing, and a reader that could not tell this from a
    /// declined receipt would conclude the ledger rejected work it never saw.
    pub unreadable: usize,
}

/// How many receipts this service is holding, and whether that is all of them.
///
/// A PARTIAL COUNT THAT SAID SO IS USABLE; ONE THAT DID NOT IS WORSE THAN
/// NOTHING. The retention bound can stop the channel being emptied, and a
/// caller told only a number would read the number as the whole obligation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PrivateInputReceiptInventory {
    /// Receipts in this service's own custody.
    pub retained: usize,
    /// Whether the delivery channel was emptied into that custody. When false,
    /// `retained` is a floor and not a total.
    pub complete: bool,
}

impl PrivateInputHandle {
    /// Take the delivery receipts that have arrived, without waiting.
    ///
    /// DRAINED, NEVER PROBED AWAY. Each call returns what is queued and leaves
    /// nothing behind; asking whether anything is there is the same act as
    /// taking it, so there is no separate question that consumes.
    pub fn drain_deliveries(&self) -> Result<PrivateInputReceipts, PrivateInputUnavailable> {
        self.consume_receipts(None)
    }

    /// Take delivery receipts, waiting up to this bound for the first one.
    pub fn drain_deliveries_within(
        &self,
        within: Duration,
    ) -> Result<PrivateInputReceipts, PrivateInputUnavailable> {
        self.consume_receipts(Some(within))
    }

    /// Take receipts and hand each one back to the ledger that issued it.
    ///
    /// A RECEIPT IS NOT CONSUMED BY BEING READ. A delivery's ticket is pruned
    /// only once it is both routing-finished and observed, so a receipt that is
    /// popped off the channel and merely handed to a caller leaves its place
    /// taken for good. Enough of those and the service stops accepting
    /// deliveries without having grown by a byte: the bound is on tickets, not
    /// on the channel. Observing here is what gives the place back, and it
    /// reuses the ledger's own ticket rather than counting anything twice.
    ///
    /// AN UNOBSERVED RECEIPT IS KEPT. The ledger refuses an observation whose
    /// ticket it does not hold, one already made, and one whose terminal
    /// answer is not the receipt offered. None of those is a reason to drop the
    /// receipt: it is retained, offered again on the next drain, and counted as
    /// owed at stop.
    fn consume_receipts(
        &self,
        within: Option<Duration>,
    ) -> Result<PrivateInputReceipts, PrivateInputUnavailable> {
        // EVERY FALLIBLE ACQUISITION BEFORE ANY OBSERVATION. An earlier version
        // observed the retained receipts first and then reached for the
        // delivery channel, so a poisoned channel returned `Err` and took the
        // already-observed receipts with it: their places were given back in
        // the ledger and the caller was never told which receipts those were.
        // Both locks are taken here, and after that nothing can fail.
        let mut retained = self
            .runtime
            .retained_receipts
            .lock()
            .map_err(|_| PrivateInputUnavailable)?;
        let held = self
            .runtime
            .deliveries
            .lock()
            .map_err(|_| PrivateInputUnavailable)?;

        // TAKEN INTO CUSTODY BEFORE ANYTHING IS OBSERVED. A receipt moved out
        // of the channel and into this queue has changed owner and nothing
        // else; it is not consumed, not observed, and still owed.
        let room = PRIVATE_INPUT_RECEIPT_RETENTION.saturating_sub(retained.len());
        if room > 0 {
            let mut taken = Vec::new();
            if let Some(within) = within
                && let Ok(first) = held.recv_timeout(within)
            {
                taken.push(first);
            }
            taken.extend(held.try_iter().take(room - taken.len()));
            retained.extend(taken);
        }
        drop(held);

        // BOUNDED BY WHAT THIS CALL VISITS, AND VISITED IN PLACE. Counting the
        // remainder let a full queue be observed and then a full channel
        // drained on top, which is twice the bound this advertises. Walking the
        // whole queue to rebuild it would also make the work proportional to
        // everything held rather than to the bound, so only the prefix is
        // touched and only released receipts are removed.
        let mut observed = Vec::new();
        let mut unreadable = 0usize;
        let limit = retained.len().min(PRIVATE_INPUT_DRAIN_BOUND);
        let mut visited = 0usize;
        let mut index = 0usize;
        while visited < limit {
            let receipt = retained[index];
            visited += 1;
            match self.runtime.observer.observe(receipt) {
                sophia_x_authority::PrivateDeliveryObservation::Observed => {
                    retained.remove(index);
                    observed.push(receipt);
                }
                sophia_x_authority::PrivateDeliveryObservation::Unreadable => {
                    // NOT A REFUSAL. The ledger was never asked, so this is
                    // counted apart from the receipts it actually declined.
                    unreadable += 1;
                    index += 1;
                }
                _ => index += 1,
            }
        }

        Ok(PrivateInputReceipts {
            observed,
            retained: retained.len(),
            visited,
            unreadable,
        })
    }

    /// Take queued receipts into custody and report what is owed.
    ///
    /// IT MOVES RECEIPTS, AND IT HAS TO. A count that read only the retained
    /// queue reported zero while receipts sat unread in the delivery channel,
    /// each one still holding its ticket's place; the channel cannot be
    /// measured without taking from it. Nothing here is observed, so nothing
    /// is consumed: the receipts change owner from the channel to this
    /// service, which is where they were going to have to be counted anyway.
    ///
    /// IT SAYS WHEN IT COULD NOT TAKE EVERYTHING. The retention bound can stop
    /// it short of emptying the channel, and a total reported in that case
    /// would be a total of what fitted rather than of what is owed. The answer
    /// carries whether the channel was actually emptied, so a partial reading
    /// can never be mistaken for a complete one.
    pub fn unobserved_receipts(
        &self,
    ) -> Result<PrivateInputReceiptInventory, PrivateInputUnavailable> {
        let mut retained = self
            .runtime
            .retained_receipts
            .lock()
            .map_err(|_| PrivateInputUnavailable)?;
        let held = self
            .runtime
            .deliveries
            .lock()
            .map_err(|_| PrivateInputUnavailable)?;
        let room = PRIVATE_INPUT_RECEIPT_RETENTION.saturating_sub(retained.len());
        let taken: Vec<_> = held.try_iter().take(room).collect();
        // Fewer than there was room for means the channel ran out, which is the
        // only way to know it is empty. Taking exactly the room available
        // leaves the question open, and so does having no room at all.
        let complete = taken.len() < room;
        retained.extend(taken);
        Ok(PrivateInputReceiptInventory {
            retained: retained.len(),
            complete,
        })
    }

    /// Take the control acknowledgements that have arrived, without waiting.
    ///
    /// DRAINED, NEVER PROBED AWAY. Each call returns what is queued and leaves
    /// nothing behind; asking whether anything is there is the same act as
    /// taking it, so there is no separate question that consumes.
    pub fn drain_acknowledgements(&self) -> Vec<XAuthorityClientControlAck> {
        self.runtime
            .acknowledgements
            .lock()
            .map(|held| held.try_iter().take(PRIVATE_INPUT_DRAIN_BOUND).collect())
            .unwrap_or_default()
    }

    /// Take control acknowledgements, waiting up to this bound for the first one.
    pub fn drain_acknowledgements_within(
        &self,
        within: Duration,
    ) -> Vec<XAuthorityClientControlAck> {
        let Ok(held) = self.runtime.acknowledgements.lock() else {
            return Vec::new();
        };
        let mut taken = Vec::new();
        if let Ok(first) = held.recv_timeout(within) {
            taken.push(first);
        }
        taken.extend(
            held.try_iter()
                .take(PRIVATE_INPUT_DRAIN_BOUND - taken.len()),
        );
        taken
    }

    /// Take the observed transaction batches that have arrived, without waiting.
    ///
    /// DRAINED, NEVER PROBED AWAY. Each call returns what is queued and leaves
    /// nothing behind; asking whether anything is there is the same act as
    /// taking it, so there is no separate question that consumes.
    pub fn drain_transactions(&self) -> Vec<XAuthorityObservedTransactionBatch> {
        self.runtime
            .transactions
            .lock()
            .map(|held| held.try_iter().take(PRIVATE_INPUT_DRAIN_BOUND).collect())
            .unwrap_or_default()
    }

    /// Take observed transaction batches, waiting up to this bound for the first one.
    pub fn drain_transactions_within(
        &self,
        within: Duration,
    ) -> Vec<XAuthorityObservedTransactionBatch> {
        self.drain_transactions_limited(within, PRIVATE_INPUT_DRAIN_BOUND)
    }

    /// Take at most `limit` batches, however long the drain bound is.
    ///
    /// THE CALLER'S REMAINING ROOM IS THE LIMIT THAT MATTERS. Asking whether
    /// there was any room and then taking a whole drain bound's worth is how a
    /// bounded queue reaches half again its bound: room for one is not room for
    /// two hundred and fifty six.
    pub fn drain_transactions_limited(
        &self,
        within: Duration,
        limit: usize,
    ) -> Vec<XAuthorityObservedTransactionBatch> {
        self.try_drain_transactions_limited(within, limit)
            .unwrap_or_default()
    }

    /// Take at most `limit` batches, or say the channel could not be read.
    ///
    /// UNREADABLE IS NOT AN EMPTY CHANNEL. The infallible form above answers
    /// both with an empty list, so a caller driving commits treats a poisoned
    /// receiver exactly as it treats a quiet one: it reports a step that
    /// observed nothing and moves on, while intake it can no longer reach
    /// accumulates behind the lock. A caller that must not do that uses this.
    pub(super) fn try_drain_transactions_limited(
        &self,
        within: Duration,
        limit: usize,
    ) -> Result<Vec<XAuthorityObservedTransactionBatch>, PrivateInputUnavailable> {
        let limit = limit.min(PRIVATE_INPUT_DRAIN_BOUND);
        if limit == 0 {
            return Ok(Vec::new());
        }
        let held = self
            .runtime
            .transactions
            .lock()
            .map_err(|_| PrivateInputUnavailable)?;
        let mut taken = Vec::new();
        if let Ok(first) = held.recv_timeout(within) {
            taken.push(first);
        }
        taken.extend(held.try_iter().take(limit - taken.len()));
        Ok(taken)
    }
}
