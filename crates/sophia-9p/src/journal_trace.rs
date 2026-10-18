//! Opt-in, bounded journal identity trace for comparator diagnostics. Not a
//! cost sampler: enable it only in diagnostic captures, never in compared
//! timing, CPU or allocation runs.
//!
//! Records journal identity only: sequences, byte ranges, read deliveries and
//! acknowledgements, plus identities a role supplies. No payload bytes, clock
//! or principal data. `begin` reserves all storage; recording takes one
//! uncontended lock and pushes a plain value into that reservation, so it
//! never allocates, formats or performs I/O. Events beyond the reservation are
//! counted, not stored. An entry's position is its ordinal: one total order
//! across every journal and mark.
//!
//! Only the thread that called `begin` records, until it calls `take`, so
//! concurrent tests in one process stay out of each other's traces. A role
//! that services its journals on that thread is traced whole.
use std::cell::Cell;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

/// What one read request returned.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReadResult {
    /// Bytes, possibly none for a zero-count read.
    Ready,
    /// Below the released floor.
    Stale,
    /// Past the end offset.
    Invalid,
}

/// What one acknowledgement did.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AckResult {
    /// Released retention through the requested sequence.
    Advanced,
    /// Repeated the last acknowledgement; released nothing.
    Repeat,
    /// Named another epoch.
    Stale,
    /// Named a sequence below the last acknowledgement or not retained.
    Invalid,
}

/// Where a role reports a record's identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RolePhase {
    /// The role appended a record of `kind` at `sequence`.
    Appended,
    /// The role accepted a client submission; `sequence` is its receipt.
    Submitted,
    /// The role published a record answering some earlier request.
    Outcome,
    /// The role released submission custody: `sequence` is the receipt and
    /// `related` the covering acknowledgement.
    Released,
}

/// A role-defined join key. `class` names what `id` and `generation` mean,
/// so the 9P layer carries no role vocabulary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RoleKey {
    pub class: &'static str,
    pub id: u64,
    pub generation: u64,
}

impl RoleKey {
    pub const NONE: Self = Self {
        class: "",
        id: 0,
        generation: 0,
    };
}

/// One identity a role reports for a journal record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RoleRecord {
    pub journal: u64,
    pub sequence: u64,
    pub phase: RolePhase,
    /// The role's record kind.
    pub kind: u16,
    /// The role's transaction identity, or 0.
    pub transaction: u64,
    pub key: RoleKey,
    /// The role's status or outcome code, or 0.
    pub status: u16,
    /// Phase-specific: the covering acknowledgement for `Released`, the
    /// client's submission identity for `Submitted`, otherwise 0.
    pub related: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Event {
    /// A journal was created; `journal` identifies it for this process.
    Opened {
        journal: u64,
        epoch: u64,
        next_sequence: u64,
        tail: u64,
    },
    /// Records `first..=last` were published at bytes `start..end`.
    Commit {
        journal: u64,
        first: u64,
        last: u64,
        start: u64,
        end: u64,
    },
    /// One completed read. `first..=last` are the records it touched (0 when
    /// none). `starts_inside` and `ends_inside` mark a first record entered
    /// past its start and a last record left before its end. `parked` counts
    /// reads of this journal that found no bytes since its previous
    /// completed read.
    Read {
        journal: u64,
        offset: u64,
        requested: u32,
        delivered: u32,
        first: u64,
        last: u64,
        starts_inside: bool,
        ends_inside: bool,
        result: ReadResult,
        parked: u32,
    },
    /// One acknowledgement. `acknowledged` and `floor` are after it;
    /// `last_committed` is the highest sequence published at the time.
    Ack {
        journal: u64,
        epoch: u64,
        requested: u64,
        result: AckResult,
        acknowledged: u64,
        floor: u64,
        last_committed: u64,
    },
    Role(RoleRecord),
    /// A driver-supplied boundary, such as the start of an owner turn.
    Mark {
        label: u64,
    },
}

struct Trace {
    events: Vec<Event>,
    dropped: u64,
}

static TRACE: Mutex<Option<Trace>> = Mutex::new(None);
static JOURNALS: AtomicU64 = AtomicU64::new(1);

thread_local! {
    static TRACING: Cell<bool> = const { Cell::new(false) };
}

/// Start a trace of this thread holding at most `capacity` events.
pub fn begin(capacity: usize) {
    let mut trace = TRACE.lock().unwrap();
    assert!(trace.is_none(), "journal trace already active");
    *trace = Some(Trace {
        events: Vec::with_capacity(capacity),
        dropped: 0,
    });
    TRACING.with(|tracing| tracing.set(true));
}

/// End this thread's trace: its events in ordinal order, and how many did
/// not fit.
pub fn take() -> (Vec<Event>, u64) {
    assert!(TRACING.with(Cell::get), "trace taken by its own thread");
    TRACING.with(|tracing| tracing.set(false));
    let trace = TRACE.lock().unwrap().take().expect("active journal trace");
    (trace.events, trace.dropped)
}

pub fn record(event: Event) {
    if !TRACING.with(Cell::get) {
        return;
    }
    let mut trace = TRACE.lock().unwrap();
    if let Some(trace) = trace.as_mut() {
        if trace.events.len() < trace.events.capacity() {
            trace.events.push(event);
        } else {
            trace.dropped += 1;
        }
    }
}

/// A driver boundary. A mark directly after another replaces it, so a
/// driver may mark every turn of a busy loop: only turns that recorded
/// something keep their mark, and storage stays bounded by real events.
pub fn mark(label: u64) {
    if !TRACING.with(Cell::get) {
        return;
    }
    let mut trace = TRACE.lock().unwrap();
    if let Some(trace) = trace.as_mut() {
        if let Some(Event::Mark { label: last }) = trace.events.last_mut() {
            *last = label;
        } else if trace.events.len() < trace.events.capacity() {
            trace.events.push(Event::Mark { label });
        } else {
            trace.dropped += 1;
        }
    }
}

pub fn role(record_value: RoleRecord) {
    record(Event::Role(record_value));
}

/// A process-unique journal identity; epochs repeat across roles.
pub(crate) fn open(epoch: u64, next_sequence: u64, tail: u64) -> u64 {
    let journal = JOURNALS.fetch_add(1, Ordering::Relaxed);
    record(Event::Opened {
        journal,
        epoch,
        next_sequence,
        tail,
    });
    journal
}

/// The records one read touches, gathered inside the read loop.
#[derive(Default)]
pub(crate) struct ReadSpan {
    first: u64,
    last: u64,
    starts_inside: bool,
    ends_inside: bool,
}

impl ReadSpan {
    pub(crate) fn touch(&mut self, sequence: u64, entered_at: usize, left_at: usize, len: usize) {
        if self.first == 0 {
            self.first = sequence;
            self.starts_inside = entered_at != 0;
        }
        self.last = sequence;
        self.ends_inside = left_at != len;
    }
}

pub(crate) fn read(
    journal: u64,
    offset: u64,
    requested: u32,
    delivered: u32,
    span: ReadSpan,
    result: ReadResult,
    parked: u32,
) {
    record(Event::Read {
        journal,
        offset,
        requested,
        delivered,
        first: span.first,
        last: span.last,
        starts_inside: span.starts_inside,
        ends_inside: span.ends_inside,
        result,
        parked,
    });
}
