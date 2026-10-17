//! Per-unit custody: what became of each unit a caller handed the connection.
//!
//! Admission into the outbox is local queue ownership, not custody. On the
//! file wire a record reaches custody only when the session's `Submitted`
//! event for its submission is observed, and resource bytes when their slot
//! write returns `Rwrite`. The socket wire proves nothing beyond the kernel
//! accepting the bytes, so its units end at [`Custody::Written`].

use std::collections::VecDeque;

/// Identifies one unit (one record, or one resource-chunk write) for the
/// life of its connection. Tickets rise strictly and are never reused.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Ticket(pub u64);

/// Why a connection cannot report a ticket's custody. Tickets are local to
/// one connection; the same number in another connection is unrelated.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CustodyLookupError {
    /// Zero or a number this connection has not issued.
    Unissued,
    /// Issued, but older than the retained 256 tickets. This says nothing
    /// about whether the server accepted the operation.
    Unavailable,
}

/// The tickets one admission issued, in wire order. A pair (an action ACK and
/// the activation it authorizes) is two consecutive tickets.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Admission {
    pub first: Ticket,
    pub count: u32,
}

impl Admission {
    pub fn tickets(self) -> impl Iterator<Item = Ticket> {
        (self.first.0..self.first.0 + u64::from(self.count)).map(Ticket)
    }
}

/// What is known about one unit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Custody {
    /// Admitted locally; not yet handed to the wire.
    Queued,
    /// Handed to the wire; its outcome is not yet known.
    InFlight,
    /// File wire: the session's `Submitted` event for this record was
    /// observed. Custody only, not a semantic outcome, which follows as its
    /// own event.
    Submitted,
    /// File wire: the slot write carrying these resource bytes returned
    /// `Rwrite` for all of them.
    Stored,
    /// Socket wire: the kernel accepted every byte. No custody is proven.
    Written,
    /// The session definitively refused the unit with this wire (Linux)
    /// error number; nothing was journaled.
    Refused(u32),
    /// The connection ended before the unit left the client.
    DroppedUnsent,
    /// The connection ended after the unit was submitted but before its
    /// custody was observed. It may or may not have been accepted.
    Unknown,
}

/// The most recent tickets' outcomes this ledger keeps; an older ticket's
/// outcome becomes unavailable rather than being confused with a newer one.
pub(crate) const LEDGER_CAPACITY: usize = 256;

pub(crate) struct Ledger {
    next: u64,
    /// `(ticket, custody)` for the newest tickets, oldest first.
    recent: VecDeque<(u64, Custody)>,
}

impl Default for Ledger {
    fn default() -> Self {
        Self {
            next: 1,
            recent: VecDeque::new(),
        }
    }
}

impl Ledger {
    /// Acquire storage before an application commits its local side effect.
    pub(crate) fn reserve(&mut self, count: usize) -> Result<(), crate::ShellClientError> {
        let needed = self.recent.len().saturating_add(count).min(LEDGER_CAPACITY);
        self.recent
            .try_reserve(needed - self.recent.len())
            .map_err(|_| crate::ShellClientError::QueueSaturated)
    }

    /// Issues `count` consecutive tickets, all `Queued`.
    pub(crate) fn issue(&mut self, count: usize) -> Admission {
        let first = self.next;
        for offset in 0..count as u64 {
            if self.recent.len() == LEDGER_CAPACITY {
                self.recent.pop_front();
            }
            self.recent.push_back((first + offset, Custody::Queued));
        }
        self.next = first + count as u64;
        Admission {
            first: Ticket(first),
            count: count as u32,
        }
    }

    /// The first ticket the next `issue` would return.
    pub(crate) fn peek(&self) -> u64 {
        self.next
    }

    pub(crate) fn get(&self, ticket: Ticket) -> Option<Custody> {
        let oldest = self.recent.front()?.0;
        let index = ticket.0.checked_sub(oldest)? as usize;
        self.recent
            .get(index)
            .filter(|(issued, _)| *issued == ticket.0)
            .map(|(_, custody)| *custody)
    }

    pub(crate) fn lookup(&self, ticket: Ticket) -> Result<Custody, CustodyLookupError> {
        if ticket.0 == 0 || ticket.0 >= self.next {
            return Err(CustodyLookupError::Unissued);
        }
        self.get(ticket).ok_or(CustodyLookupError::Unavailable)
    }

    /// Records a new state. An evicted ticket is ignored: its outcome is
    /// simply no longer reported. A settled outcome is final: nothing later
    /// replaces `Submitted` (or any other settled state) for the same unit.
    pub(crate) fn set(&mut self, ticket: u64, custody: Custody) {
        let Some(oldest) = self.recent.front().map(|(issued, _)| *issued) else {
            return;
        };
        let Some(index) = ticket.checked_sub(oldest) else {
            return;
        };
        if let Some(entry) = self.recent.get_mut(index as usize)
            && entry.0 == ticket
            && matches!(entry.1, Custody::Queued | Custody::InFlight)
        {
            entry.1 = custody;
        }
    }
}

#[cfg(test)]
#[path = "../tests/support/custody.rs"]
mod tests;
