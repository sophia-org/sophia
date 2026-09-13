//! One ordered stream for everything a private instance has to sequence.
//!
//! Five queues visited in a new loop is still five queues: each advances at
//! its own rate, and operations from different ones interleave differently on
//! every pass. A single stream exists so that ordering is a property of
//! admission rather than of which loop happened to run first.
//!
//! Position is assigned and the entry is published in the same action. A
//! design that took an ordinal and published afterwards would leave a hole
//! between the two, and a consumer reaching that hole has to either block on
//! work that may never arrive or skip a position that later fills.

use std::collections::VecDeque;

/// What kind of operation an entry carries.
///
/// The class exists for capacity policy, not for ordering: everything is
/// ordered together, and nothing may jump.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadyClass {
    /// Input admitted through the stamped envelope.
    RoutedInput,
    /// Work a grab had frozen, becoming runnable again.
    Thawed,
    /// Control whose application belongs in the same order.
    Control,
    /// A connection-side change to grabs, freezes or focus.
    ConnectionMutation,
    /// Privileged cleanup, which outlives grants.
    Cleanup,
}

impl ReadyClass {
    /// Whether this class may draw on capacity kept back for cleanup.
    ///
    /// Cleanup that cannot be admitted is worse than input that cannot: input
    /// refused is a caller told no, while cleanup refused is state nobody will
    /// come back to release.
    const fn may_use_reserve(self) -> bool {
        matches!(self, Self::Cleanup)
    }
}

/// Why the stream refused an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadyRefusal {
    /// No capacity remains for this class.
    AtCapacity,
    /// Sequence numbers are exhausted; none may be reused.
    SequencesExhausted,
}

/// Where an admitted entry sits in the single order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ReadySequence(u64);

impl ReadySequence {
    pub fn raw(self) -> u64 {
        self.0
    }
}

/// The single ordered stream.
///
/// Entries leave in the order they were admitted, so a producer that admits
/// its own work in order sees that order preserved. Work that was delayed
/// takes its position when it becomes runnable, not when it was first
/// requested: a position held from request time would reorder it ahead of work
/// that was runnable throughout.
#[derive(Debug)]
pub struct ReadyStream<T> {
    entries: VecDeque<(ReadySequence, ReadyClass, T)>,
    next_sequence: u64,
    capacity: usize,
    cleanup_reserve: usize,
}

impl<T> ReadyStream<T> {
    /// A stream bounded at `capacity`, of which `cleanup_reserve` entries are
    /// kept back for privileged cleanup.
    ///
    /// Storage for the whole capacity is taken here, so admission never has to
    /// allocate: an entry is accepted only once there is somewhere to put it.
    pub fn new(capacity: usize, cleanup_reserve: usize) -> Self {
        let cleanup_reserve = cleanup_reserve.min(capacity);
        Self {
            entries: VecDeque::with_capacity(capacity),
            next_sequence: 1,
            capacity,
            cleanup_reserve,
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// How many entries this class may still admit.
    pub fn remaining_for(&self, class: ReadyClass) -> usize {
        let ceiling = if class.may_use_reserve() {
            self.capacity
        } else {
            self.capacity.saturating_sub(self.cleanup_reserve)
        };
        ceiling.saturating_sub(self.entries.len())
    }

    /// Assign this entry its position and publish it, as one action.
    ///
    /// There is no separate reservation step, and deliberately so: the
    /// sequence a caller is told is a sequence the consumer can already see.
    pub fn admit(&mut self, class: ReadyClass, payload: T) -> Result<ReadySequence, ReadyRefusal> {
        if self.remaining_for(class) == 0 {
            return Err(ReadyRefusal::AtCapacity);
        }
        let sequence = ReadySequence(self.next_sequence);
        // Checked before publishing, so a stream that can no longer name its
        // entries refuses rather than reusing a name an earlier one answers to.
        self.next_sequence = next_ready_sequence(self.next_sequence)?;
        self.entries.push_back((sequence, class, payload));
        Ok(sequence)
    }

    /// Take the next entry in order.
    pub fn take_next(&mut self) -> Option<(ReadySequence, ReadyClass, T)> {
        self.entries.pop_front()
    }
}

/// The position an entry after this one would take.
///
/// A rule that stands on its own, so exhaustion can be shown without a
/// constructor that puts a stream into a state it could never reach by
/// admitting. Wrapping here would hand out a position an earlier entry still
/// answers to, which is the whole reason positions exist.
pub fn next_ready_sequence(current: u64) -> Result<u64, ReadyRefusal> {
    current
        .checked_add(1)
        .ok_or(ReadyRefusal::SequencesExhausted)
}
