//! G3-5d d-2b3d-1 Session controls: publication and journal evidence for
//! the accepted custody rows (R4, P1-P3, F1-F3) and F4's internal
//! preparation, over the same lane, driver turns and triggers as the
//! accepted controls. Every owner fact is read between turns through the
//! slot-indexed observers (`transport_facts`, `store_event_facts`,
//! `owned_releasing`, `candidate_prepared`): reading drains, publishes,
//! ACKs, flushes, services and advances nothing. The five stages are kept
//! apart: a pending store event, an outbox entry, a journal commit
//! (identified sequence and byte range, from the retained table), the
//! journal's visible end, and the peer's actual consumption (its record
//! log and journal progress).
//!
//! Calls are named where used. T1 is the accepted row's turn. PP0 is one
//! peer-only poll before T1 that the accepted rows do not make: it drains
//! any reply already answered and re-arms the peer's one `events` read, and
//! fixes the reply-witness baseline. PP1/PP2 are peer-only polls; IT is
//! `input_turn`, the labelled transport pass `service_owner_turn` (REV2
//! D5/section 7 answer 1): it serves the peer's input and also drains,
//! flushes and applies ACKs, and its effects are recorded, not assumed
//! absent. A window is the records committed by one turn, identified by
//! sequence and byte range from the snapshot taken immediately after it.
//!
//! H5 is the explanation under test, not a premise: after a turn, the peer
//! can read only the range the export's one Ready reply returned in that
//! turn. The witness is export-produced; the peer's decode of the same
//! range (PP1's new records) establishes delivery. A missing or different
//! witness leaves H5 Absent/Refuted and establishes nothing else.
//!
//! Predictions: the exact record kinds and order of every window, the P1
//! Closed record, the P2 Submitted record, the F3 terminal reason and the
//! Submitted-free lean commits are predictions until this harness runs; a
//! miss is a reported failure. The status-before-terminal order in P1 is
//! observed and printed, not asserted. Session off/native-session remain
//! unpassed and unwaived. F4's later presentation outcome needs the queued
//! fallback's completion: that is b3d-2 F4-O, declared separately, and no
//! empty-list assertion here stands for it.
use super::*;
use sophia_protocol::shell_files::{ShellFileKind, ShellFileSubmitted};
use sophia_runtime::owned_ring_protocol::{Event, EventKind, Identity};
use sophia_runtime::{
    JournalFacts, OutboxRecordFacts, OwnedCreditFacts, RecordIdentity, RetainedEntry,
    RetainedRecord, StoreEventFacts, TransportFacts,
};
use sophia_shell_client::JournalProgress;

/// The lane's stream open (30) and the P1 invalidation (50); clear of the
/// commits (40 + n).
const OPEN: TransactionId = TransactionId::from_raw(30);
const CLOSE: TransactionId = TransactionId::from_raw(31);
const INVALIDATE: TransactionId = TransactionId::from_raw(50);
const LOST: (u16, u16) = (3, ContentReason::AllocationLost as u16);
const FAILED: (u16, u16) = (3, ContentReason::RendererFailed as u16);
const MALFORMED: (u16, u16) = (3, ContentReason::Malformed as u16);

/// One read of slot 0 between turns.
struct Snapshot {
    facts: TransportFacts,
    stores: StoreEventFacts,
    releasing: Vec<Identity>,
}

impl Snapshot {
    fn journal(&self) -> &JournalFacts {
        self.facts
            .journal
            .as_ref()
            .expect("a wired slot has a journal")
    }

    /// The last Ready reply's serial, 0 before any.
    fn serial(&self) -> u64 {
        self.journal().ready.map_or(0, |witness| witness.serial)
    }

    fn assert_stores_empty(&self, label: &str) {
        assert!(
            self.stores.resources.is_empty()
                && self.stores.candidates.is_empty()
                && self.stores.allocations.is_empty(),
            "{label}: queued store events {:?}",
            self.stores
        );
    }
}

/// The commit-side facts an owner-free step leaves unchanged.
fn committed(journal: &JournalFacts) -> (u64, u64, u64, Option<(u64, u64)>) {
    (
        journal.next_sequence,
        journal.tail,
        journal.visible,
        journal.deferred,
    )
}

impl Lane {
    /// Slot 0's facts under `a`'s connected attempt, read without a
    /// service borrow.
    fn snapshot(&self, a: &Attempt, label: &str) -> Snapshot {
        let processes = &self.h.owner.processes;
        assert_eq!(
            processes.attempt(0),
            Some(a.key),
            "{label}: slot 0 holds the attempt"
        );
        let (attempt, facts) = processes.transport_facts(0).expect("slot 0 exists");
        assert_eq!(
            attempt,
            Some((a.key.grant, ComponentConnectionPhase::Connected)),
            "{label}: connected under the attempt's grant"
        );
        assert!(facts.wired, "{label}: the file wire is held");
        let stores = processes
            .store_event_facts(0)
            .unwrap_or_else(|| panic!("{label}: the store grant has an active epoch"));
        let releasing = processes
            .owned_releasing(0)
            .unwrap_or_else(|| panic!("{label}: slot 0 has an image owner"));
        Snapshot {
            facts,
            stores,
            releasing,
        }
    }

    /// IT: one `service_owner_turn` on `a`'s connection, a transport pass
    /// (input service, drain, flush, ACKs), not a pure read.
    fn input_turn(&mut self, a: &Attempt) {
        self.h
            .owner
            .with_service(a.key, |_, t| t.service_owner_turn())
            .unwrap()
            .unwrap();
    }

    /// PP1, O2, IT, PP2, O3 after T1's snapshot `s1`, with the H5 verdict
    /// for T1 against the pre-T1 serial baseline in `s0`. Returns the
    /// verdict, PP1's and PP2's new peer records, and O3.
    fn schedule(
        &mut self,
        a: &mut Attempt,
        s0: &Snapshot,
        s1: &Snapshot,
        label: &str,
    ) -> (H5, Vec<PeerRecord>, Vec<PeerRecord>, Snapshot) {
        let before = a.log.len();
        let pp1 = peer_poll(a);
        let l1 = a.log[before..].to_vec();
        let s2 = self.snapshot(a, label);
        assert_eq!(
            s2.journal(),
            s1.journal(),
            "{label} O2: a peer-only poll changes no owner journal fact"
        );
        assert_eq!(s2.facts.outbox, s1.facts.outbox, "{label} O2: outbox");
        self.input_turn(a);
        let it = self.snapshot(a, &format!("{label} after IT"));
        assert_eq!(
            (it.journal().next_sequence, it.journal().tail),
            (s1.journal().next_sequence, s1.journal().tail),
            "{label} IT: the transport pass committed nothing"
        );
        let before = a.log.len();
        let pp2 = peer_poll(a);
        let l2 = a.log[before..].to_vec();
        let s3 = self.snapshot(a, &format!("{label} O3"));
        let verdict = h5(s0.serial(), s1, &l1, label);
        eprintln!(
            "{label}: PP1 {pp1:?} L1 {l1:?}; IT ready {:?} pending {}; PP2 {pp2:?} L2 {l2:?}; H5 {}",
            it.journal().ready,
            it.journal().pending_evaluations,
            verdict.describe()
        );
        (verdict, l1, l2, s3)
    }
}

/// One peer-only poll, then the peer's new records into `a`.
fn peer_poll(a: &mut Attempt) -> JournalProgress {
    a.peer.poll_io().unwrap();
    observe_peer(a);
    a.peer.journal_progress()
}

/// The records one turn committed: those in `[before.tail, after.tail)`
/// of `after`'s table, which must carry exactly the sequences
/// `[before.next_sequence, after.next_sequence)` with no incomplete or
/// undecodable entry among them. Retention must still cover the window.
fn window(before: &Snapshot, after: &Snapshot, label: &str) -> Vec<RetainedRecord> {
    let (b, a) = (before.journal(), after.journal());
    assert_eq!(b.epoch, a.epoch, "{label}: one epoch");
    assert!(b.tail <= a.tail, "{label}: the tail never moves back");
    assert!(
        a.floor <= b.tail,
        "{label}: retention floor {} passed the window start {}",
        a.floor,
        b.tail
    );
    let mut records = Vec::new();
    for entry in &after.facts.table {
        let start = match entry {
            RetainedEntry::Record(record) => record.start,
            RetainedEntry::Incomplete { start, .. } | RetainedEntry::Undecodable { start, .. } => {
                *start
            }
        };
        if start < b.tail || start >= a.tail {
            continue;
        }
        match entry {
            RetainedEntry::Record(record) => records.push(record.clone()),
            other => panic!("{label}: {other:?} inside the window"),
        }
    }
    let sequences: Vec<u64> = records.iter().map(|record| record.sequence).collect();
    let expected: Vec<u64> = (b.next_sequence..a.next_sequence).collect();
    assert_eq!(sequences, expected, "{label}: the window's sequences");
    match (records.first(), records.last()) {
        (Some(first), Some(last)) => {
            assert_eq!(
                first.start, b.tail,
                "{label}: the window starts at T's tail"
            );
            assert_eq!(last.end, a.tail, "{label}: the window ends at the new tail");
        }
        _ => eprintln!("{label}: empty window (vacuous)"),
    }
    eprintln!("{label}: window {:?}", kinds(&records));
    records
}

fn status_of(record: &RetainedRecord) -> Option<&ContentResourceStatus> {
    match &record.identity {
        RecordIdentity::Content {
            record: ShellContentRecord::ResourceStatus(status),
            ..
        } => Some(status),
        _ => None,
    }
}

fn allocation_result_of(
    record: &RetainedRecord,
) -> Option<(TransactionId, &ContentAllocationResult)> {
    match &record.identity {
        RecordIdentity::Content {
            transaction,
            record: ShellContentRecord::AllocationResult(result),
        } => Some((*transaction, result)),
        _ => None,
    }
}

fn outcome_of(record: &RetainedRecord) -> Option<&ContentCandidateOutcome> {
    match &record.identity {
        RecordIdentity::Frame(ShellFrameRecord::Outcome { outcome, .. }) => Some(outcome),
        _ => None,
    }
}

fn closed_of(record: &RetainedRecord) -> Option<(TransactionId, ShellFrameStreamKey, u16)> {
    match &record.identity {
        RecordIdentity::Frame(ShellFrameRecord::Closed {
            transaction,
            key,
            reason,
        }) => Some((*transaction, *key, *reason)),
        _ => None,
    }
}

fn submitted_of(record: &RetainedRecord) -> Option<ShellFileSubmitted> {
    match &record.identity {
        RecordIdentity::Submitted(submitted) => Some(*submitted),
        _ => None,
    }
}

fn slot_released_of(record: &RetainedRecord) -> Option<Event> {
    match &record.identity {
        RecordIdentity::OwnedEvent(event) if event.kind == EventKind::SlotReleased => Some(*event),
        _ => None,
    }
}

/// Short labels for a window, for exact-order assertions.
fn kinds(records: &[RetainedRecord]) -> Vec<String> {
    records
        .iter()
        .map(|record| match &record.identity {
            RecordIdentity::Content {
                record: ShellContentRecord::ResourceStatus(status),
                ..
            } => format!("ResourceStatus({})", status.status),
            RecordIdentity::Content {
                record: ShellContentRecord::AllocationResult(_),
                ..
            } => "AllocationResult".into(),
            RecordIdentity::Content { record, .. } => format!("Content({record:?})"),
            RecordIdentity::Frame(ShellFrameRecord::Outcome { outcome, .. }) => {
                format!("FrameOutcome({},{})", outcome.kind, outcome.reason)
            }
            RecordIdentity::Frame(ShellFrameRecord::Closed { .. }) => "Closed".into(),
            RecordIdentity::Frame(frame) => format!("Frame({:?})", frame.kind()),
            RecordIdentity::Submitted(_) => "Submitted".into(),
            RecordIdentity::OwnedEvent(event) => format!("{:?}({})", event.kind, event.reason),
            RecordIdentity::Other => format!("Other({:?})", record.kind),
        })
        .collect()
}

/// Candidate `n`'s frame outcomes in a window, as (kind, reason).
fn terminals_in(records: &[RetainedRecord], n: u64) -> Vec<(u16, u16)> {
    records
        .iter()
        .filter_map(outcome_of)
        .filter(|outcome| outcome.output == OUTPUT && outcome.candidate_generation == n)
        .map(|outcome| (outcome.kind, outcome.reason))
        .collect()
}

/// The window's one ResourceStatus for the lane's resource.
fn one_status<'a>(records: &'a [RetainedRecord], label: &str) -> &'a RetainedRecord {
    let statuses: Vec<&RetainedRecord> = records
        .iter()
        .filter(|record| status_of(record).is_some_and(|status| status.resource == RESOURCE))
        .collect();
    assert_eq!(
        statuses.len(),
        1,
        "{label}: one ResourceStatus for the resource in {:?}",
        kinds(records)
    );
    statuses[0]
}

/// What the peer surfaces for a retained record, if anything: Submitted
/// and owned-ring events are consumed inside the client.
fn peer_record(record: &RetainedRecord) -> Option<PeerRecord> {
    match &record.identity {
        RecordIdentity::Content {
            transaction,
            record,
        } => Some(PeerRecord::Content(*transaction, record.clone())),
        RecordIdentity::Frame(frame) => Some(PeerRecord::Frame(frame.clone())),
        RecordIdentity::Submitted(_) | RecordIdentity::OwnedEvent(_) | RecordIdentity::Other => {
            None
        }
    }
}

/// The H5 verdict for one turn: exactly one new Ready reply in the turn,
/// whose returned range decodes to exactly PP1's new records.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum H5 {
    /// The reply's records are PP1's records; `untruncated` when the reply
    /// reached the then-visible end.
    Supported {
        untruncated: bool,
    },
    /// No reply and nothing delivered.
    Vacuous,
    /// The turn produced no Ready reply (nothing attributable).
    Absent,
    /// More than one Ready reply in the turn: not attributable to one.
    Insufficient(u64),
    Refuted,
}

impl H5 {
    fn describe(self) -> String {
        match self {
            Self::Supported { untruncated } => format!("Supported (untruncated {untruncated})"),
            Self::Vacuous => "Vacuous".into(),
            Self::Absent => "Absent".into(),
            Self::Insufficient(replies) => format!("Insufficient ({replies} replies)"),
            Self::Refuted => "Refuted".into(),
        }
    }
}

fn h5(serial_before: u64, s1: &Snapshot, delivered: &[PeerRecord], label: &str) -> H5 {
    let journal = s1.journal();
    let Some(witness) = journal.ready.filter(|w| w.serial > serial_before) else {
        return if delivered.is_empty() {
            H5::Absent
        } else {
            eprintln!("{label}: records delivered with no new Ready reply: {delivered:?}");
            H5::Refuted
        };
    };
    if witness.serial != serial_before + 1 {
        return H5::Insufficient(witness.serial - serial_before);
    }
    assert_eq!(witness.epoch, journal.epoch, "{label}: the reply's epoch");
    let end = witness.offset + witness.returned as u64;
    let expected: Vec<PeerRecord> = s1
        .facts
        .table
        .iter()
        .filter_map(|entry| match entry {
            RetainedEntry::Record(record)
                if record.start >= witness.offset && record.end <= end =>
            {
                peer_record(record)
            }
            _ => None,
        })
        .collect();
    eprintln!(
        "{label}: Ready serial {} [{}, {end}) of visible {} (requested {}, clamped {}): expected {expected:?}",
        witness.serial,
        witness.offset,
        witness.visible,
        witness.count_requested,
        witness.count_clamped
    );
    if expected.is_empty() && delivered.is_empty() {
        H5::Vacuous
    } else if expected == delivered {
        H5::Supported {
            untruncated: end == witness.visible,
        }
    } else {
        H5::Refuted
    }
}

/// The one live credit for the lane's resource.
fn resource_credit(s: &Snapshot, label: &str) -> OwnedCreditFacts {
    let credits: Vec<OwnedCreditFacts> = s
        .facts
        .owned_live_credits
        .iter()
        .copied()
        .filter(|credit| credit.offer.request.resource == RESOURCE)
        .collect();
    assert_eq!(
        credits.len(),
        1,
        "{label}: one live credit for the resource in {:?}",
        s.facts.owned_live_credits
    );
    credits[0]
}

/// R4 baseline, corrected from the carried prediction: the accepted
/// status does not wait in the T1 outbox; T1's content service drains it
/// into the journal as a deferred commit (the lane is lean and the frame
/// is in flight), invisible to reads. SlotReleased is refused in T1
/// (release.rs: the status reservation is unspent when the images step
/// runs) and commits in T2's images step after the status, in the same
/// deferred run. Neither turn answers the peer's read. Submitted is not a
/// flip.
///
/// MD7's broader consumer (REV4 qualification 1): under the guard
/// deletion, T1's images step would commit SlotReleased before the
/// status; the first reachable assertion is T1's window, which must be
/// exactly one ResourceStatus. The full-journal consumer is b3d-2's
/// separate observation, not promised here.
#[test]
fn r4_the_accepted_status_commits_deferred_in_t1_and_slot_released_follows_it_in_t2() {
    let (mut lane, wake) = Lane::running();
    let mut a = lane.attempt();
    let _job = rendered(&mut lane, &wake, &mut a);
    let pp0 = peer_poll(&mut a);
    let s0 = lane.snapshot(&a, "R4 O0");
    eprintln!("R4 PP0 {pp0:?}; O0 journal {:?}", s0.journal());
    let credit = resource_credit(&s0, "R4 O0");
    assert!(
        credit.status_reserved,
        "the status reservation is unspent before T1"
    );
    let identity = credit.offer.identity;
    assert_eq!(identity.grant, a.key.grant);
    assert!(s0.releasing.is_empty(), "no source work awaits release");
    assert_eq!(
        s0.facts.lean_frames_in_flight,
        Some(1),
        "the frame is in flight"
    );
    let t1 = lane.result_turn();
    assert_eq!(t1.steps, TURN);
    assert_eq!(t1.accepted, [1], "the rendered frame is accepted");
    assert_eq!(t1.status, Some(Submit::SubmittedWaitingForPageFlip));
    let s1 = lane.snapshot(&a, "R4 O1");
    let w1 = window(&s0, &s1, "R4 T1");
    assert_eq!(
        kinds(&w1),
        ["ResourceStatus(2)"],
        "MD7: the release committed before its status"
    );
    let status = &w1[0];
    let settled = status_of(status).unwrap();
    assert_eq!(
        (settled.grant, settled.resource, settled.status),
        (a.key.grant, RESOURCE, 2),
        "the identified acceptance"
    );
    assert_eq!(
        s1.facts.owned_publication_sequence,
        Some(status.sequence),
        "the status is the owned ring's last publication"
    );
    assert_eq!(
        s1.releasing,
        [identity],
        "the settled source work awaits its SlotReleased"
    );
    assert_eq!(
        resource_credit(&s1, "R4 O1"),
        OwnedCreditFacts {
            offer: credit.offer,
            status_reserved: false,
        },
        "the status reservation is spent; the release reservation is kept"
    );
    let j1 = s1.journal();
    assert_eq!(
        j1.deferred,
        Some((status.start, status.end)),
        "lean: the acceptance waits for its frame's terminal"
    );
    assert_eq!(j1.visible, status.start, "invisible below it");
    assert_eq!(s1.facts.lean_frames_in_flight, Some(1));
    assert!(s1.facts.outbox.is_empty(), "{:?}", s1.facts.outbox);
    assert!(!s1.facts.output_blocked);
    s1.assert_stores_empty("R4 O1");
    assert_eq!(
        s1.serial(),
        s0.serial(),
        "no Ready reply in T1: nothing became visible"
    );
    let pp1 = peer_poll(&mut a);
    assert!(
        pp1.handled < status.sequence,
        "unread: handled {} before status {}",
        pp1.handled,
        status.sequence
    );
    assert!(a.terminals(1).is_empty());
    assert_eq!(a.accepted, 0, "the peer has not read the acceptance");
    let t2 = lane.result_turn();
    assert_eq!(t2.steps, TURN);
    assert_eq!(t2.accepted, [0], "nothing more to accept");
    let s2 = lane.snapshot(&a, "R4 O2");
    let w2 = window(&s1, &s2, "R4 T2");
    assert_eq!(
        kinds(&w2),
        ["SlotReleased(0)"],
        "T2's images step releases the slot"
    );
    let released = &w2[0];
    let event = slot_released_of(released).unwrap();
    assert_eq!(
        (event.identity, event.resource, event.slot, event.reason),
        (identity, RESOURCE, credit.offer.request.slot, 0),
        "the original source's release"
    );
    assert!(
        status.sequence < released.sequence,
        "status commit {} before SlotReleased commit {}",
        status.sequence,
        released.sequence
    );
    assert!(s2.releasing.is_empty(), "the release work is done");
    assert!(
        !s2.facts
            .owned_live_credits
            .iter()
            .any(|c| c.offer == credit.offer),
        "the slot credit is spent: {:?}",
        s2.facts.owned_live_credits
    );
    assert_eq!(s2.facts.owned_publication_sequence, Some(released.sequence));
    let j2 = s2.journal();
    assert_eq!(
        j2.deferred,
        Some((status.start, released.end)),
        "one deferred run spans both"
    );
    assert_eq!(j2.visible, status.start, "still invisible: no terminal yet");
    assert_eq!(s2.facts.lean_frames_in_flight, Some(1));
    assert_eq!(s2.serial(), s0.serial(), "no Ready reply in T2 either");
    let pp2 = peer_poll(&mut a);
    assert!(pp2.handled < status.sequence, "still unread");
    assert!(a.terminals(1).is_empty());
    assert_eq!(a.accepted, 0);
    assert_eq!(lane.releases(), (0, 0), "the head holds the lease");
    assert_eq!(lane.leased(), 1);
}

/// P1: the invalidation queues AllocationResult(50) in the outbox between
/// turns without a drain (flush_content_allocation_events); the journal is
/// unchanged. T1 commits it, the resource's refusal status and the
/// candidate's one AllocationLost terminal; the stream's Closed record for
/// the lost allocation is the predicted fourth. The result commits before
/// the status; the status/terminal order is observed. Nothing stays
/// deferred: the terminal ends the frame's flight. Then the schedule: PP1
/// reads what T1's one reply returned, IT serves the re-armed read, PP2
/// reads the rest.
#[test]
fn p1_a_lost_allocation_journals_its_result_the_refusal_and_the_terminal_in_t1() {
    let (mut lane, wake) = Lane::running();
    let mut a = lane.attempt();
    let _job = rendered(&mut lane, &wake, &mut a);
    let pp0 = peer_poll(&mut a);
    let s0 = lane.snapshot(&a, "P1 O0");
    eprintln!("P1 PP0 {pp0:?}; O0 journal {:?}", s0.journal());
    lane.h
        .owner
        .with_service(a.key, |_, t| {
            t.invalidate_content_allocation(INVALIDATE, ALLOCATION, ContentReason::AllocationLost)
        })
        .unwrap()
        .unwrap();
    let queued = lane.snapshot(&a, "P1 after invalidation");
    assert_eq!(
        committed(queued.journal()),
        committed(s0.journal()),
        "the invalidation journals nothing itself"
    );
    assert_eq!(queued.facts.outbox.len(), 1, "{:?}", queued.facts.outbox);
    let entry = &queued.facts.outbox[0];
    let OutboxRecordFacts::Content(transaction, ShellContentRecord::AllocationResult(result)) =
        &entry.record
    else {
        panic!("an AllocationResult: {entry:?}")
    };
    assert_eq!(*transaction, INVALIDATE);
    assert_eq!(result.allocation, ALLOCATION);
    assert_eq!(result.reason, ContentReason::AllocationLost as u16);
    assert!(!entry.resource_settlement);
    assert!(!queued.facts.output_blocked);
    assert!(
        queued.stores.allocations.is_empty(),
        "transferred to the outbox without a drain"
    );
    let (steps, accepted) = lane.quiet_driver_turn_at(Instant::now());
    assert_eq!(steps, TURN);
    assert_eq!(accepted, [0], "no bundle joined");
    let s1 = lane.snapshot(&a, "P1 O1");
    let w1 = window(&s0, &s1, "P1 T1");
    let result = w1
        .iter()
        .find(|record| allocation_result_of(record).is_some_and(|(t, _)| t == INVALIDATE))
        .expect("AllocationResult(50) committed in T1");
    let status = one_status(&w1, "P1 T1");
    let refusal = status_of(status).unwrap();
    assert_ne!(refusal.status, 2, "a refusal: {refusal:?}");
    assert_eq!(terminals_in(&w1, 1), [LOST], "one AllocationLost terminal");
    let terminal = w1.iter().find(|r| outcome_of(r).is_some()).unwrap();
    let closed: Vec<(ContentOutputId, ContentAllocationId, u16)> = w1
        .iter()
        .filter_map(closed_of)
        .map(|(_, key, reason)| (key.output, key.allocation, reason))
        .collect();
    assert_eq!(
        closed,
        [(OUTPUT, ALLOCATION, ContentReason::AllocationLost as u16)],
        "the stream closed with its allocation"
    );
    assert!(
        result.sequence < status.sequence,
        "the result {} commits before the status {}",
        result.sequence,
        status.sequence
    );
    eprintln!(
        "P1: status {} {} terminal {}",
        status.sequence,
        if status.sequence < terminal.sequence {
            "before"
        } else {
            "after"
        },
        terminal.sequence
    );
    let j1 = s1.journal();
    assert_eq!(j1.visible, j1.tail, "nothing deferred after the terminal");
    assert_eq!(j1.deferred, None);
    assert_eq!(s1.facts.lean_frames_in_flight, Some(0));
    assert!(s1.facts.outbox.is_empty(), "{:?}", s1.facts.outbox);
    assert!(!s1.facts.output_blocked);
    s1.assert_stores_empty("P1 O1");
    let (verdict, l1, l2, s3) = lane.schedule(&mut a, &s0, &s1, "P1");
    assert_ne!(verdict, H5::Refuted, "H5 refuted for P1");
    assert_eq!(
        l1.len() + l2.len(),
        4,
        "the peer surfaced T1's four records once"
    );
    assert_eq!(a.terminals(1), [LOST], "the terminal once");
    assert_eq!((a.accepted, a.refused), (0, 1), "the refusal once");
    assert!(!a.stream.has_credit(), "closed: {:?}", a.stream.state());
    eprintln!(
        "P1: O3 floor {} records {} (O1 floor {}); peer {:?}",
        s3.journal().floor,
        s3.journal().records,
        s1.journal().floor,
        a.peer.journal_progress()
    );
    assert_eq!(lane.releases(), (1, 1), "the walk released Held as Ended");
    lane.slot_freed("P1");
}

/// P2: the Close is read by T1's input service and admitted by its content
/// step, which still accepts the admitted frame. T1 commits the Close's
/// Submitted (FrameStreamClose is a candidate-class request), Closed under
/// the Close's own transaction with the exact key, and the frame's
/// acceptance, and no terminal for candidate 1. Whether the acceptance
/// stays deferred depends on whether it is the last of the three; the
/// peer gains Closed and, only if visible, the acceptance.
#[test]
fn p2_a_close_after_admission_journals_closed_under_its_transaction_and_no_terminal() {
    let (mut lane, wake) = Lane::running();
    let mut a = lane.attempt();
    let job = rendered(&mut lane, &wake, &mut a);
    let binding = lane.h.owner.processes.combined_row(0).binding(job).unwrap();
    a.stream.close(&mut a.peer, CLOSE).unwrap();
    let pp0 = peer_poll(&mut a);
    let s0 = lane.snapshot(&a, "P2 O0");
    eprintln!("P2 PP0 {pp0:?}; O0 journal {:?}", s0.journal());
    assert_eq!(
        lane.stream_fact(&a, binding),
        Some((FrameStreamPhase::Open, OPEN)),
        "no owner call has admitted the Close yet"
    );
    let t1 = lane.result_turn();
    assert_eq!(t1.steps, TURN);
    assert_eq!(t1.accepted, [1], "the admitted frame is accepted");
    assert_eq!(t1.status, Some(Submit::SubmittedWaitingForPageFlip));
    assert_eq!(
        lane.stream_fact(&a, binding),
        Some((FrameStreamPhase::Closing, CLOSE)),
        "Closing under the Close's transaction"
    );
    let s1 = lane.snapshot(&a, "P2 O1");
    let w1 = window(&s0, &s1, "P2 T1");
    assert!(
        terminals_in(&w1, 1).is_empty(),
        "no terminal for the admitted frame"
    );
    let submitted: Vec<ShellFileKind> = w1
        .iter()
        .filter_map(submitted_of)
        .map(|s| s.candidate_kind)
        .collect();
    assert_eq!(
        submitted,
        [ShellFileKind::FrameStreamClose],
        "the Close's own Submitted"
    );
    let closed: Vec<(TransactionId, ShellFrameStreamKey, u16)> =
        w1.iter().filter_map(closed_of).collect();
    assert_eq!(closed.len(), 1, "one Closed: {:?}", kinds(&w1));
    assert_eq!(closed[0].0, CLOSE, "under the Close's transaction");
    assert_eq!(
        closed[0].1,
        ShellFrameStreamKey {
            grant: binding.key.grant,
            output: binding.key.output,
            allocation: binding.key.allocation,
            facts_generation: binding.key.facts_generation,
            stream_id: binding.key.stream_id,
        },
        "the exact stream key"
    );
    eprintln!("P2: Closed reason {}", closed[0].2);
    let status = one_status(&w1, "P2 T1");
    assert_eq!(
        status_of(status).unwrap().status,
        2,
        "the frame's acceptance"
    );
    let j1 = s1.journal();
    let status_last = w1
        .last()
        .is_some_and(|last| last.sequence == status.sequence);
    assert_eq!(
        j1.deferred.is_some(),
        status_last,
        "the acceptance is deferred only while it is the tail: {j1:?}"
    );
    assert_eq!(s1.facts.lean_frames_in_flight, Some(1));
    assert!(s1.facts.outbox.is_empty(), "{:?}", s1.facts.outbox);
    s1.assert_stores_empty("P2 O1");
    let (verdict, l1, l2, _) = lane.schedule(&mut a, &s0, &s1, "P2");
    assert_ne!(verdict, H5::Refuted, "H5 refuted for P2");
    let delivered: Vec<&PeerRecord> = l1.iter().chain(&l2).collect();
    let closed_seen = delivered
        .iter()
        .filter(|r| matches!(r, PeerRecord::Frame(ShellFrameRecord::Closed { transaction, .. }) if *transaction == CLOSE))
        .count();
    assert_eq!(closed_seen, 1, "the peer read Closed once: {delivered:?}");
    assert!(a.terminals(1).is_empty(), "no terminal reached the peer");
    assert_eq!(
        a.accepted,
        usize::from(!status_last),
        "the acceptance reaches the peer only when visible"
    );
    assert!(!a.stream.has_credit(), "{:?}", a.stream.state());
    assert_eq!(lane.pending(&a), [(a.key.grant, OUTPUT, 1)]);
    assert_eq!(lane.releases(), (0, 0), "the head holds the lease");
    assert_eq!(lane.leased(), 1);
}

/// P3: the candidate's own expiry. T1's content service drains the
/// acceptance first (deferred), then its expiry commits the Timeout
/// terminal, which ends the deferred run: both are visible after T1 and
/// the terminal's own commit answers the peer's parked read, so one
/// peer-only poll reads both (the accepted row's `peer_reads_terminal`
/// needed no owner call). Compared with P1/P2, nothing precedes the
/// terminal that could answer the read early.
#[test]
fn p3_the_expiry_commits_the_acceptance_then_timeout_and_one_poll_reads_both() {
    let (mut lane, wake) = Lane::running();
    let mut a = lane.attempt();
    let limits = lane
        .h
        .owner
        .with_service(a.key, |_, t| t.content_limits().cloned())
        .unwrap()
        .expect("negotiated limits");
    let (candidate, transfer) = (
        u64::from(limits.candidate_timeout_ms),
        u64::from(limits.transfer_timeout_ms),
    );
    let before = content_now_msec();
    let job = rendered(&mut lane, &wake, &mut a);
    let binding = lane.h.owner.processes.combined_row(0).binding(job).unwrap();
    let deadline = lane.deadline_of(job);
    let pp0 = peer_poll(&mut a);
    let s0 = lane.snapshot(&a, "P3 O0");
    eprintln!("P3 PP0 {pp0:?}; O0 journal {:?}", s0.journal());
    advance_content_clock(candidate);
    let at = Instant::now();
    assert!(
        at < deadline,
        "the deciding turn precedes the admitted deadline"
    );
    let (steps, accepted) = lane.quiet_driver_turn_at(at);
    assert!(
        content_now_msec() < before + transfer,
        "precondition: before any transfer deadline"
    );
    assert_eq!(steps, TURN);
    assert_eq!(accepted, [0], "no bundle joined");
    let s1 = lane.snapshot(&a, "P3 O1");
    let w1 = window(&s0, &s1, "P3 T1");
    assert_eq!(
        kinds(&w1),
        ["ResourceStatus(2)", "FrameOutcome(3,6)"],
        "the acceptance, then the Timeout terminal"
    );
    assert_eq!(terminals_in(&w1, 1), [TIMEOUT]);
    let j1 = s1.journal();
    assert_eq!(j1.visible, j1.tail, "the terminal ended the deferred run");
    assert_eq!(j1.deferred, None);
    assert_eq!(s1.facts.lean_frames_in_flight, Some(0));
    assert!(s1.facts.outbox.is_empty(), "{:?}", s1.facts.outbox);
    s1.assert_stores_empty("P3 O1");
    let (verdict, l1, l2, _) = lane.schedule(&mut a, &s0, &s1, "P3");
    assert_eq!(l1.len(), 2, "one peer-only poll reads both: {l1:?}");
    assert!(l2.is_empty(), "nothing more: {l2:?}");
    assert_eq!(
        verdict,
        H5::Supported { untruncated: true },
        "T1's one reply returned both records"
    );
    assert_eq!(a.terminals(1), [TIMEOUT], "the expiry, once");
    assert_eq!((a.accepted, a.refused), (1, 0));
    assert_eq!(lane.candidate_of(&a, binding, job), Some(Unknown));
    assert_eq!(
        lane.states(),
        [(job, "held")],
        "Held kept before the deadline"
    );
    assert_eq!(lane.releases(), (0, 0));
}

/// F1: the labelled draw failure (declared before submission). T1 commits
/// the candidate's one RendererFailed terminal; the resource's status, if
/// any, is printed beside it. Nothing stays deferred.
#[test]
fn f1_the_render_failure_terminal_is_journaled_once_in_t1() {
    let (mut lane, wake) = Lane::running();
    let mut a = lane.attempt();
    lane.native.declare_render_failure();
    let _job = faulted(&mut lane, &wake, &mut a);
    assert_eq!(lane.leased(), 0, "the failed draw freed its slot");
    let pp0 = peer_poll(&mut a);
    let s0 = lane.snapshot(&a, "F1 O0");
    eprintln!("F1 PP0 {pp0:?}; O0 journal {:?}", s0.journal());
    let (steps, observed) = lane.observed_turn(Instant::now(), None);
    assert_eq!(steps, TURN);
    assert_eq!(observed, NOTHING, "content neither accepted nor released");
    let s1 = lane.snapshot(&a, "F1 O1");
    let w1 = window(&s0, &s1, "F1 T1");
    assert_eq!(
        terminals_in(&w1, 1),
        [FAILED],
        "one RendererFailed terminal"
    );
    let statuses: Vec<u16> = w1.iter().filter_map(status_of).map(|s| s.status).collect();
    eprintln!("F1: statuses in T1 {statuses:?}");
    let j1 = s1.journal();
    assert_eq!(j1.visible, j1.tail, "nothing deferred after the terminal");
    assert_eq!(s1.facts.lean_frames_in_flight, Some(0));
    assert!(s1.facts.outbox.is_empty(), "{:?}", s1.facts.outbox);
    s1.assert_stores_empty("F1 O1");
    let (verdict, _, _, _) = lane.schedule(&mut a, &s0, &s1, "F1");
    assert_ne!(verdict, H5::Refuted, "H5 refuted for F1");
    assert_eq!(a.terminals(1), [FAILED], "the terminal once");
    eprintln!(
        "F1: peer statuses accepted {} refused {}",
        a.accepted, a.refused
    );
    assert_eq!(lane.releases(), (0, 0), "nothing was leased to release");
}

/// F2: the labelled plan mismatch. Kept separate: the worker's validation
/// error (plan_mismatches), the copy's Malformed answer, the resource's
/// refusal status and the candidate's Malformed terminal, both committed
/// in T1, and the peer's reads.
#[test]
fn f2_the_mismatch_journals_the_refusal_status_and_the_malformed_terminal() {
    let (mut lane, wake) = Lane::running();
    let mut a = lane.attempt();
    lane.native.declare_plan_mismatch();
    let job = faulted(&mut lane, &wake, &mut a);
    assert_eq!(
        lane.native.plan_mismatches(),
        (1, 0),
        "the validation error"
    );
    let binding = lane.h.owner.processes.combined_row(0).binding(job).unwrap();
    let pp0 = peer_poll(&mut a);
    let s0 = lane.snapshot(&a, "F2 O0");
    eprintln!("F2 PP0 {pp0:?}; O0 journal {:?}", s0.journal());
    let (steps, observed) = lane.observed_turn(Instant::now(), None);
    assert_eq!(steps, TURN);
    assert_eq!(observed, NOTHING, "no bundle met the failed entry");
    let s1 = lane.snapshot(&a, "F2 O1");
    let w1 = window(&s0, &s1, "F2 T1");
    let status = one_status(&w1, "F2 T1");
    let refusal = status_of(status).unwrap();
    assert_ne!(refusal.status, 2, "the copy's refusal: {refusal:?}");
    assert_eq!(terminals_in(&w1, 1), [MALFORMED], "one Malformed terminal");
    let terminal = w1.iter().find(|r| outcome_of(r).is_some()).unwrap();
    eprintln!(
        "F2: status {} {} terminal {}",
        status.sequence,
        if status.sequence < terminal.sequence {
            "before"
        } else {
            "after"
        },
        terminal.sequence
    );
    let j1 = s1.journal();
    assert_eq!(j1.visible, j1.tail, "nothing deferred after the terminal");
    assert_eq!(s1.facts.lean_frames_in_flight, Some(0));
    s1.assert_stores_empty("F2 O1");
    let (verdict, _, _, _) = lane.schedule(&mut a, &s0, &s1, "F2");
    assert_ne!(verdict, H5::Refuted, "H5 refuted for F2");
    assert_eq!(a.terminals(1), [MALFORMED], "the terminal once");
    assert_eq!((a.accepted, a.refused), (0, 1), "the refusal once");
    assert_eq!(lane.candidate_of(&a, binding, job), Some(Unknown));
    assert_eq!(lane.states(), [(job, "failed")]);
    assert_eq!(lane.releases(), (0, 0), "no lease, no Release");
}

/// F3: C3, the arm's own admitted-deadline decline. T1 commits the
/// acceptance and the candidate's terminal (predicted RendererFailed, the
/// arm's decline reason); then, after slot freedom, T2's images step
/// commits SlotReleased after the status, as R4, visible at once since
/// the frame's flight ended.
#[test]
fn f3_the_c3_decline_journals_the_status_and_terminal_then_slot_released() {
    let (mut lane, wake) = Lane::running();
    let mut a = lane.attempt();
    let candidate = lane.candidate_timeout(&a);
    let before = content_now_msec();
    let job = rendered(&mut lane, &wake, &mut a);
    let late = lane.deadline_of(job) + MARGIN;
    let pp0 = peer_poll(&mut a);
    let s0 = lane.snapshot(&a, "F3 O0");
    eprintln!("F3 PP0 {pp0:?}; O0 journal {:?}", s0.journal());
    let credit = resource_credit(&s0, "F3 O0");
    let (steps, observed) = lane.observed_turn(late, None);
    assert!(
        content_now_msec() < before + candidate,
        "precondition: before the candidate's own deadline"
    );
    assert_eq!(steps, TURN);
    assert_eq!(observed, DECLINED, "C3 declined at the content call");
    assert_eq!(lane.native.classifies(), 0, "C3 precedes classification");
    let s1 = lane.snapshot(&a, "F3 O1");
    let w1 = window(&s0, &s1, "F3 T1");
    assert_eq!(
        kinds(&w1),
        ["ResourceStatus(2)", "FrameOutcome(3,9)"],
        "the acceptance, then the decline's terminal"
    );
    let status = &w1[0];
    assert_eq!(terminals_in(&w1, 1), [FAILED]);
    let j1 = s1.journal();
    assert_eq!(j1.visible, j1.tail, "the terminal ended the deferred run");
    assert_eq!(s1.facts.lean_frames_in_flight, Some(0));
    assert_eq!(s1.releasing, [credit.offer.identity], "release owed");
    s1.assert_stores_empty("F3 O1");
    let (verdict, _, _, s3) = lane.schedule(&mut a, &s0, &s1, "F3");
    assert_ne!(verdict, H5::Refuted, "H5 refuted for F3");
    assert_eq!(a.terminals(1), [FAILED], "the terminal once");
    assert_eq!((a.accepted, a.refused), (1, 0));
    assert_eq!(lane.releases(), (1, 1));
    lane.slot_freed("F3");
    let (steps, observed) = lane.observed_turn(late, None);
    assert_eq!(steps, TURN);
    assert_eq!(observed, DECLINED, "nothing more");
    let s2 = lane.snapshot(&a, "F3 O2");
    let w2 = window(&s3, &s2, "F3 T2");
    assert_eq!(kinds(&w2), ["SlotReleased(0)"]);
    let released = &w2[0];
    assert_eq!(
        slot_released_of(released).unwrap().identity,
        credit.offer.identity
    );
    assert!(
        status.sequence < released.sequence,
        "status {} before SlotReleased {}",
        status.sequence,
        released.sequence
    );
    let j2 = s2.journal();
    assert_eq!(j2.visible, j2.tail, "published at once: no frame in flight");
    assert!(s2.releasing.is_empty());
    assert_eq!(lane.native.installs(), 0);
    assert_eq!(lane.acquisitions(), 1, "no later render");
}

/// F4-P: scene churn, the result declined into one ordinary entry. The
/// candidate is prepared internally (`prepared_without_notice`: the lane
/// is terminal-only) and no Prepared record (kind 1) for candidate 1 is
/// committed, while the acceptance commits deferred, invisible until a
/// terminal. The ordinary entry's presentation outcome needs the queued
/// fallback's completion: b3d-2 F4-O, not asserted here.
#[test]
fn f4p_scene_churn_prepares_the_candidate_internally_without_a_prepared_record() {
    let (mut lane, wake) = Lane::running();
    let mut a = lane.attempt();
    let _job = rendered(&mut lane, &wake, &mut a);
    assert!(lane.native.ordinary_queue_ready(output().id));
    let pp0 = peer_poll(&mut a);
    let s0 = lane.snapshot(&a, "F4-P O0");
    eprintln!("F4-P PP0 {pp0:?}; O0 journal {:?}", s0.journal());
    assert_eq!(
        lane.h.owner.processes.candidate_prepared(0, OUTPUT, 1),
        None,
        "assembling: not yet a pending candidate"
    );
    let (steps, observed) = lane.observed_turn(Instant::now(), MOVED);
    assert_eq!(steps, TURN);
    assert_eq!(observed, DECLINED, "declined before the walk");
    assert_eq!(lane.native.classifies(), 1);
    assert_eq!(lane.native.installs(), 1, "one ordinary install entry");
    assert_eq!(lane.native.ordinary_queues(), 1);
    let s1 = lane.snapshot(&a, "F4-P O1");
    let w1 = window(&s0, &s1, "F4-P T1");
    assert!(
        !w1.iter().any(|r| outcome_of(r)
            .is_some_and(|o| { o.output == OUTPUT && o.candidate_generation == 1 && o.kind == 1 })),
        "no Prepared record for candidate 1: {:?}",
        kinds(&w1)
    );
    assert!(terminals_in(&w1, 1).is_empty(), "no terminal");
    assert_eq!(
        lane.h.owner.processes.candidate_prepared(0, OUTPUT, 1),
        Some(true),
        "prepared internally, notice suppressed"
    );
    assert_eq!(kinds(&w1), ["ResourceStatus(2)"], "the acceptance alone");
    let status = &w1[0];
    let j1 = s1.journal();
    assert_eq!(j1.deferred, Some((status.start, status.end)), "deferred");
    assert_eq!(j1.visible, status.start, "invisible");
    assert_eq!(s1.facts.lean_frames_in_flight, Some(1));
    assert_eq!(lane.pending(&a), [(a.key.grant, OUTPUT, 1)]);
    let (verdict, l1, l2, _) = lane.schedule(&mut a, &s0, &s1, "F4-P");
    assert!(
        l1.is_empty() && l2.is_empty(),
        "nothing visible to read: {l1:?} {l2:?}"
    );
    assert_eq!(verdict, H5::Absent, "no reply in T1");
    assert!(a.terminals(1).is_empty());
    assert_eq!(a.accepted, 0);
    lane.slot_freed("churn");
}
