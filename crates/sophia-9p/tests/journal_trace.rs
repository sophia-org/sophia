//! The journal trace records exact commit ranges, read spans, parked reads and
//! acknowledgement results. One test: the trace is process-global.
#![cfg(feature = "comparator-journal-trace")]
use sophia_9p::journal::{Journal, JournalBounds};
use sophia_9p::journal_trace::{
    self as trace, AckResult, Event, ReadResult, RoleKey, RolePhase, RoleRecord,
};
use sophia_9p::{Errno, ReadOutcome};

const BOUNDS: JournalBounds = JournalBounds {
    records: 16,
    bytes: 4096,
};

fn ready(outcome: Result<ReadOutcome, Errno>) -> usize {
    match outcome.unwrap() {
        ReadOutcome::Ready(bytes) => bytes.len(),
        ReadOutcome::Pending => panic!("pending"),
    }
}

#[test]
fn records_exact_identity_without_changing_journal_results() {
    // Nothing is recorded, and nothing fails, while no trace is active.
    let mut untraced = Journal::new(1);
    untraced.prepare(vec![0; 4], BOUNDS).unwrap().commit();

    trace::begin(64);
    let mut journal = Journal::starting_at(7, 5, 100);
    let id = journal.trace_id();
    assert_ne!(id, untraced.trace_id(), "journal identities are unique");
    assert_eq!(journal.prepare(vec![1; 10], BOUNDS).unwrap().commit(), 5);
    assert_eq!(
        journal
            .prepare_batch(vec![vec![2; 6], vec![3; 4]], BOUNDS)
            .unwrap()
            .commit(),
        7
    );
    // A dropped reservation publishes and records nothing.
    drop(journal.prepare(vec![4; 8], BOUNDS).unwrap());
    // Turns that recorded nothing leave no mark.
    trace::mark(0);
    trace::mark(1);
    // Reads at the end park without an event; the next completed read
    // reports how many parked.
    assert!(matches!(journal.read(120, 64), Ok(ReadOutcome::Pending)));
    assert!(matches!(journal.read(120, 64), Ok(ReadOutcome::Pending)));
    // 100..113 covers record 5 whole and leaves record 6 at 3 of 6 bytes.
    assert_eq!(ready(journal.read(100, 13)), 13);
    // The continuation enters record 6 past its start and ends record 7.
    assert_eq!(ready(journal.read(113, 64)), 7);
    // An explicit reread of record 5 is recorded as such, not merged.
    assert_eq!(ready(journal.read(100, 10)), 10);
    assert_eq!(ready(journal.read(100, 0)), 0);
    assert_eq!(journal.read(99, 4).unwrap_err(), Errno::ESTALE);
    assert_eq!(journal.read(121, 4).unwrap_err(), Errno::EINVAL);
    assert_eq!(journal.ack(7, 5), Ok(true));
    assert_eq!(journal.ack(7, 5), Ok(false));
    assert_eq!(journal.ack(8, 6).unwrap_err(), Errno::ESTALE);
    assert_eq!(journal.ack(7, 4).unwrap_err(), Errno::EINVAL);
    assert_eq!(journal.ack(7, 9).unwrap_err(), Errno::EINVAL);
    let role = RoleRecord {
        journal: id,
        sequence: 6,
        phase: RolePhase::Outcome,
        kind: 35,
        transaction: 9,
        key: RoleKey {
            class: "candidate_generation",
            id: 3,
            generation: 0,
        },
        status: 2,
        related: 0,
    };
    trace::role(role);
    // Other threads record nothing into this thread's trace.
    std::thread::spawn(|| {
        trace::mark(9);
        Journal::new(2)
            .prepare(vec![5; 4], BOUNDS)
            .unwrap()
            .commit();
    })
    .join()
    .unwrap();
    let (events, dropped) = trace::take();
    assert_eq!(dropped, 0);
    let read =
        |offset, requested, delivered, first, last, starts_inside, ends_inside, result, parked| {
            Event::Read {
                journal: id,
                offset,
                requested,
                delivered,
                first,
                last,
                starts_inside,
                ends_inside,
                result,
                parked,
            }
        };
    let ack = |epoch, requested, result, acknowledged, floor| Event::Ack {
        journal: id,
        epoch,
        requested,
        result,
        acknowledged,
        floor,
        last_committed: 7,
    };
    assert_eq!(
        events,
        vec![
            Event::Opened {
                journal: id,
                epoch: 7,
                next_sequence: 5,
                tail: 100
            },
            Event::Commit {
                journal: id,
                first: 5,
                last: 5,
                start: 100,
                end: 110
            },
            Event::Commit {
                journal: id,
                first: 6,
                last: 7,
                start: 110,
                end: 120
            },
            Event::Mark { label: 1 },
            read(100, 13, 13, 5, 6, false, true, ReadResult::Ready, 2),
            read(113, 64, 7, 6, 7, true, false, ReadResult::Ready, 0),
            read(100, 10, 10, 5, 5, false, false, ReadResult::Ready, 0),
            read(100, 0, 0, 0, 0, false, false, ReadResult::Ready, 0),
            read(99, 4, 0, 0, 0, false, false, ReadResult::Stale, 0),
            read(121, 4, 0, 0, 0, false, false, ReadResult::Invalid, 0),
            ack(7, 5, AckResult::Advanced, 5, 110),
            ack(7, 5, AckResult::Repeat, 5, 110),
            ack(8, 6, AckResult::Stale, 5, 110),
            ack(7, 4, AckResult::Invalid, 5, 110),
            ack(7, 9, AckResult::Invalid, 5, 110),
            Event::Role(role),
        ]
    );

    // Storage is reserved up front; overflow is counted, never grown.
    trace::begin(2);
    trace::mark(1);
    trace::role(role);
    trace::mark(2);
    let (events, dropped) = trace::take();
    assert_eq!(events.len(), 2);
    assert_eq!(dropped, 1);
}
