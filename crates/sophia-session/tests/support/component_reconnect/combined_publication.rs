//! G3-5d d-2b3d-1 Session controls: publication and journal evidence for
//! the accepted custody rows (R4, P1-P3, F1-F3) and F4's internal
//! preparation, over the same lane, driver turns and triggers as the
//! accepted controls. Every owner fact is read between turns through the
//! slot-indexed observers (`transport_facts`, `store_event_facts`,
//! `owned_releasing`, `candidate_prepared`): reading drains, publishes,
//! ACKs, flushes, services and advances nothing. The five stages are kept
//! apart: a pending store event, an outbox entry, a journal commit
//! (identified sequence and byte range, from the retained table), the
//! journal's visible end, and the peer's actual consumption (its per-queue
//! records and journal progress).
//!
//! Calls are named where used. T1 is the accepted row's turn. PP0 is one
//! peer-only poll before T1 that the accepted rows do not make: it drains
//! any reply already answered and re-arms the peer's one `events` read, and
//! fixes the reply-witness baseline. PP1/PP2 are peer-only polls; IT is
//! `input_turn`, the labelled transport pass `service_owner_turn` (REV2
//! D5/section 7 answer 1): it serves the peer's input and also drains,
//! flushes and applies ACKs, and its effects are recorded, not assumed
//! absent. A window is the records committed by one turn, identified by
//! sequence, epoch and contiguous byte range from the snapshot taken
//! immediately after it.
//!
//! Identity: candidate records join on the exact admitted key recorded
//! before T1 (grant, output, allocation, facts generation, stream id and
//! candidate generation), resource statuses on the attempt's grant and the
//! lane's resource, and a Submitted on the peer's own ticket custody.
//!
//! H5 is the explanation under test, not a premise: after a turn, the peer
//! can read only the range the export's one Ready reply returned in that
//! turn. The witness is export-produced. Delivery is per-queue evidence:
//! the peer's decoder surfaces frame and content records through separate
//! APIs, drained whole one after the other, so PP1's new records are
//! compared with the same per-kind projection of the reply range, never as
//! one cross-kind order. Supported needs the retained table to cover the
//! whole reply range in the reply's epoch with contiguous complete records,
//! the peer's handled sequence to equal the reply's last record (Submitted
//! and owned-ring records included, which the client consumes itself) and
//! each queue to match; untruncated needs the reply to end at the then
//! visible end with fewer bytes than requested. Anything less is
//! Unresolved, with its reason, and establishes nothing else.
//!
//! Deferred visibility (files/journal.rs): only a reservation-consuming
//! commit touches the marker; an ordinary append leaves a deferred run's
//! marker stored but stale, and `visible` applies it only while its end is
//! the tail. FrameStreamClosed zeroes the lean in-flight count (export.rs).
//! The rows assert the tail relation and the marker their traced order predicts.
//!
//! Predictions: every window's record kinds and order, the P1 Closed, the
//! P2 Submitted, the F3 reason, the Submitted-free lean commits, P3's IT
//! commit and F3's two-record T2 are predictions until this harness runs;
//! a miss is a reported failure. The P1 status/terminal and P2
//! status/Closed orders are observed and printed, the marker prediction
//! conditioned on them. Off/native-session remain unpassed and unwaived.
//! F4's later presentation outcome is b3d-2 F4-O, declared separately; no
//! empty-list assertion stands for it. The b3d-2 C3 filler design's Timeout
//! is corrected to RendererFailed, F3's traced reason.
//!
//! Three release facts stay apart: the owned ring's SlotReleased (the
//! source slot's obligation), the content store's ResourceReleased (the
//! backing's retirement, a journal commit that is neither peer consumption
//! nor a renderer acknowledgement) and the renderer lease (`releases`,
//! `slot_freed`). The store's release joins its resource's admission on the
//! transaction the owner gives an owned-ring upload: the offer id
//! (epoch_registry/owned_ring.rs `begin_isb`).
use super::*;
use sophia_protocol::shell_files::{ShellFileKind, ShellFileSubmitted};
use sophia_runtime::owned_ring_protocol::{Event, EventKind, Identity};
use sophia_runtime::{
    JournalFacts, OutboxRecordFacts, OwnedCreditFacts, RecordIdentity, RetainedEntry,
    RetainedRecord, StoreEventFacts, TransportFacts,
};
use sophia_shell_client::{Custody, JournalProgress};

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

    /// Every Submitted body's submission id in the retained table (the
    /// journal envelope's id is always 0; the id lives in the body).
    fn submission_ids(&self) -> Vec<u64> {
        self.facts
            .table
            .iter()
            .filter_map(|entry| match entry {
                RetainedEntry::Record(record) => submitted_of(record).map(|s| s.submission_id),
                _ => None,
            })
            .collect()
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

/// The tail relation (REV HOLD F3): no deferred run ends at the tail, so
/// everything is visible; the stored marker may be stale.
fn assert_all_visible(journal: &JournalFacts, label: &str) {
    assert_eq!(journal.visible, journal.tail, "{label}: all visible");
    assert!(
        journal.deferred.is_none_or(|(_, end)| end < journal.tail),
        "{label}: no deferred run at the tail: {journal:?}"
    );
    eprintln!("{label}: stored marker {:?}", journal.deferred);
}

/// One peer-only poll's new records, per decoder queue, and the peer's
/// handled sequence after it.
#[derive(Clone, Debug, Eq, PartialEq)]
struct Delivered {
    frames: Vec<ShellFrameRecord>,
    contents: Vec<(TransactionId, ShellContentRecord)>,
    handled: u64,
}

impl Delivered {
    fn len(&self) -> usize {
        self.frames.len() + self.contents.len()
    }

    fn is_empty(&self) -> bool {
        self.len() == 0
    }
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

    /// The exact admitted key of `job`, recorded while the job is in the row.
    fn admitted_key(&self, job: u64) -> (ShellFrameStreamKey, u64) {
        let binding = self
            .h
            .owner
            .processes
            .combined_row(0)
            .binding(job)
            .expect("the job is in the row");
        (
            ShellFrameStreamKey {
                grant: binding.key.grant,
                output: binding.key.output,
                allocation: binding.key.allocation,
                facts_generation: binding.key.facts_generation,
                stream_id: binding.key.stream_id,
            },
            binding.generation,
        )
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
    /// for T1 against the pre-T1 serial baseline in `s0`. `it` is the
    /// row's exact expectation of what the transport pass commits: the
    /// identified content records, in order, or nothing. A nonempty `it`
    /// also requires PP2 to read exactly those records and no frame, with
    /// the peer's handled sequence at the last of them; H5 is judged on
    /// T1's reply and PP1 alone. Returns the verdict, PP1's and PP2's new
    /// peer records, and O3.
    fn schedule(
        &mut self,
        a: &mut Attempt,
        s0: &Snapshot,
        s1: &Snapshot,
        it: &[(TransactionId, ShellContentRecord)],
        label: &str,
    ) -> (H5, Delivered, Delivered, Snapshot) {
        let (pp1, l1) = peer_poll(a);
        let s2 = self.snapshot(a, label);
        assert_eq!(
            s2.journal(),
            s1.journal(),
            "{label} O2: a peer-only poll changes no owner journal fact"
        );
        assert_eq!(s2.facts.outbox, s1.facts.outbox, "{label} O2: outbox");
        self.input_turn(a);
        let si = self.snapshot(a, &format!("{label} after IT"));
        let wi = window(&s2, &si, &format!("{label} IT"));
        let expected: Vec<RecordIdentity> = it
            .iter()
            .map(|(transaction, record)| RecordIdentity::Content {
                transaction: *transaction,
                record: record.clone(),
            })
            .collect();
        assert_eq!(
            wi.iter().map(|r| r.identity.clone()).collect::<Vec<_>>(),
            expected,
            "{label} IT: the transport pass commits exactly the row's expectation"
        );
        let (pp2, l2) = peer_poll(a);
        if let Some(last) = wi.last() {
            assert_eq!(l2.contents, it, "{label} PP2: exactly the IT commits");
            assert!(l2.frames.is_empty(), "{label} PP2: no frame: {l2:?}");
            assert_eq!(
                pp2.handled, last.sequence,
                "{label} PP2: handled the last IT commit"
            );
        }
        let s3 = self.snapshot(a, &format!("{label} O3"));
        let verdict = h5(s0.serial(), s1, &l1, label);
        eprintln!(
            "{label}: PP1 {pp1:?} L1 {l1:?}; IT ready {:?} pending {} window {:?}; PP2 {pp2:?} L2 {l2:?}; H5 {}",
            si.journal().ready,
            si.journal().pending_evaluations,
            kinds(&wi),
            verdict.describe()
        );
        (verdict, l1, l2, s3)
    }
}

/// One peer-only poll, then the peer's new records per queue.
fn peer_poll(a: &mut Attempt) -> (JournalProgress, Delivered) {
    let (frames, contents) = (a.frames.len(), a.contents.len());
    a.peer.poll_io().unwrap();
    observe_peer(a);
    let progress = a.peer.journal_progress();
    (
        progress,
        Delivered {
            frames: a.frames[frames..].to_vec(),
            contents: a.contents[contents..].to_vec(),
            handled: progress.handled,
        },
    )
}

fn entry_start(entry: &RetainedEntry) -> u64 {
    match entry {
        RetainedEntry::Record(record) => record.start,
        RetainedEntry::Incomplete { start, .. } | RetainedEntry::Undecodable { start, .. } => {
            *start
        }
    }
}

/// The records one turn committed: those in `[before.tail, after.tail)`
/// of `after`'s table, which must carry exactly the sequences
/// `[before.next_sequence, after.next_sequence)` in `after`'s epoch,
/// contiguous from the old tail to the new, with no incomplete or
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
    let mut next = b.tail;
    for entry in &after.facts.table {
        let start = entry_start(entry);
        if start < b.tail || start >= a.tail {
            continue;
        }
        let RetainedEntry::Record(record) = entry else {
            panic!("{label}: {entry:?} inside the window")
        };
        assert_eq!(
            record.epoch, a.epoch,
            "{label}: record {} epoch",
            record.sequence
        );
        assert_eq!(
            record.start, next,
            "{label}: record {} is contiguous",
            record.sequence
        );
        next = record.end;
        records.push(record.clone());
    }
    assert_eq!(next, a.tail, "{label}: the window ends at the new tail");
    let sequences: Vec<u64> = records.iter().map(|record| record.sequence).collect();
    let expected: Vec<u64> = (b.next_sequence..a.next_sequence).collect();
    assert_eq!(sequences, expected, "{label}: the window's sequences");
    if records.is_empty() {
        eprintln!("{label}: empty window (vacuous)");
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

fn frame_of(record: &RetainedRecord) -> Option<&ShellFrameRecord> {
    match &record.identity {
        RecordIdentity::Frame(frame) => Some(frame),
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
            RecordIdentity::Content {
                record: ShellContentRecord::ResourceReleased(_),
                ..
            } => "ResourceReleased".into(),
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

/// A frame outcome's (kind, reason) when it names exactly the admitted
/// `key` and candidate `n`: grant, output, stream, allocation, facts
/// generation and candidate generation all joined.
fn outcome_for(frame: &ShellFrameRecord, key: ShellFrameStreamKey, n: u64) -> Option<(u16, u16)> {
    match frame {
        ShellFrameRecord::Outcome {
            outcome,
            stream_id,
            allocation,
            facts_generation,
            ..
        } if outcome.grant == key.grant
            && outcome.output == key.output
            && *stream_id == key.stream_id
            && *allocation == key.allocation
            && *facts_generation == key.facts_generation
            && outcome.candidate_generation == n =>
        {
            Some((outcome.kind, outcome.reason))
        }
        _ => None,
    }
}

/// A Closed's (transaction, reason) when it names exactly `key`.
fn closed_for(frame: &ShellFrameRecord, key: ShellFrameStreamKey) -> Option<(TransactionId, u16)> {
    match frame {
        ShellFrameRecord::Closed {
            transaction,
            key: closed,
            reason,
        } if *closed == key => Some((*transaction, *reason)),
        _ => None,
    }
}

/// A window's frame records, for the same joins as the peer's.
fn frames_in(records: &[RetainedRecord]) -> Vec<ShellFrameRecord> {
    records.iter().filter_map(frame_of).cloned().collect()
}

/// Candidate `n`'s exact-key frame outcomes the peer surfaced.
fn peer_terminals(
    frames: &[ShellFrameRecord],
    key: ShellFrameStreamKey,
    n: u64,
) -> Vec<(u16, u16)> {
    frames
        .iter()
        .filter_map(|frame| outcome_for(frame, key, n))
        .collect()
}

/// Exact-key Closed records the peer surfaced.
fn peer_closed(frames: &[ShellFrameRecord], key: ShellFrameStreamKey) -> Vec<(TransactionId, u16)> {
    frames
        .iter()
        .filter_map(|frame| closed_for(frame, key))
        .collect()
}

/// The window's one ResourceStatus under `grant` for the lane's resource.
fn one_status<'a>(
    records: &'a [RetainedRecord],
    grant: ContentGrant,
    label: &str,
) -> &'a RetainedRecord {
    let statuses: Vec<&RetainedRecord> = records
        .iter()
        .filter(|record| {
            status_of(record)
                .is_some_and(|status| status.grant == grant && status.resource == RESOURCE)
        })
        .collect();
    assert_eq!(
        statuses.len(),
        1,
        "{label}: one ResourceStatus for the grant's resource in {:?}",
        kinds(records)
    );
    statuses[0]
}

/// The (status, reason) of every status the peer surfaced under `grant`
/// for the lane's resource.
fn peer_statuses(
    contents: &[(TransactionId, ShellContentRecord)],
    grant: ContentGrant,
) -> Vec<(u16, u16)> {
    contents
        .iter()
        .filter_map(|(_, record)| match record {
            ShellContentRecord::ResourceStatus(status)
                if status.grant == grant && status.resource == RESOURCE =>
            {
                Some((status.status, status.reason))
            }
            _ => None,
        })
        .collect()
}

/// The H5 verdict for one turn: exactly one new Ready reply in the turn,
/// whose returned range is covered by the retained table and decodes,
/// per queue, to exactly PP1's new records.
#[derive(Clone, Debug, Eq, PartialEq)]
enum H5 {
    /// The reply's records are PP1's records; `untruncated` when the reply
    /// reached the then-visible end and returned less than requested.
    Supported {
        untruncated: bool,
    },
    /// An empty reply and nothing delivered.
    Vacuous,
    /// The turn produced no Ready reply (nothing attributable).
    Absent,
    /// More than one Ready reply in the turn: not attributable to one.
    Insufficient(u64),
    /// The reply is not established well enough to compare: its reason.
    Unresolved(String),
    Refuted,
}

impl H5 {
    fn describe(&self) -> String {
        match self {
            Self::Supported { untruncated } => format!("Supported (untruncated {untruncated})"),
            Self::Vacuous => "Vacuous".into(),
            Self::Absent => "Absent".into(),
            Self::Insufficient(replies) => format!("Insufficient ({replies} replies)"),
            Self::Unresolved(why) => format!("Unresolved ({why})"),
            Self::Refuted => "Refuted".into(),
        }
    }

    /// Neither refuted nor unresolved; the rows' minimum.
    fn assert_established(&self, label: &str) {
        assert!(
            !matches!(self, Self::Refuted | Self::Unresolved(_)),
            "{label}: H5 {}",
            self.describe()
        );
    }
}

fn h5(serial_before: u64, s1: &Snapshot, l1: &Delivered, label: &str) -> H5 {
    let journal = s1.journal();
    let Some(witness) = journal.ready.filter(|w| w.serial > serial_before) else {
        return if l1.is_empty() {
            H5::Absent
        } else {
            eprintln!("{label}: records delivered with no new Ready reply: {l1:?}");
            H5::Refuted
        };
    };
    if witness.serial != serial_before + 1 {
        return H5::Insufficient(witness.serial - serial_before);
    }
    let unresolved = |why: String| {
        eprintln!("{label}: H5 unresolved: {why}");
        H5::Unresolved(why)
    };
    if witness.epoch != journal.epoch {
        return unresolved(format!(
            "reply epoch {} in journal epoch {}",
            witness.epoch, journal.epoch
        ));
    }
    let Some(end) = witness.offset.checked_add(witness.returned as u64) else {
        return unresolved(format!("reply range overflows: {witness:?}"));
    };
    if witness.returned == 0 {
        return if l1.is_empty() {
            H5::Vacuous
        } else {
            unresolved(format!("an empty reply, yet the peer surfaced {l1:?}"))
        };
    }
    if journal.floor > witness.offset {
        return unresolved(format!(
            "retention floor {} passed the reply offset {}",
            journal.floor, witness.offset
        ));
    }
    let mut covered: Vec<&RetainedRecord> = Vec::new();
    let mut next = witness.offset;
    for entry in &s1.facts.table {
        let start = entry_start(entry);
        if start < witness.offset || start >= end {
            continue;
        }
        let RetainedEntry::Record(record) = entry else {
            return unresolved(format!("{entry:?} inside the reply range"));
        };
        if record.start != next {
            return unresolved(format!(
                "record {} starts at {} where {next} was expected",
                record.sequence, record.start
            ));
        }
        if record.epoch != witness.epoch {
            return unresolved(format!(
                "record {} epoch {} in reply epoch {}",
                record.sequence, record.epoch, witness.epoch
            ));
        }
        next = record.end;
        covered.push(record);
    }
    if next != end {
        return unresolved(format!(
            "the reply ends at {end} but the covered records end at {next}"
        ));
    }
    let last = covered.last().expect("a nonempty covered range");
    if l1.handled != last.sequence {
        return unresolved(format!(
            "handled {} after PP1, the reply's last record is {}",
            l1.handled, last.sequence
        ));
    }
    let expected = Delivered {
        frames: covered
            .iter()
            .filter_map(|r| frame_of(r).cloned())
            .collect(),
        contents: covered
            .iter()
            .filter_map(|r| match &r.identity {
                RecordIdentity::Content {
                    transaction,
                    record,
                } => Some((*transaction, record.clone())),
                _ => None,
            })
            .collect(),
        handled: last.sequence,
    };
    eprintln!(
        "{label}: Ready serial {} [{}, {end}) of visible {} (requested {}, clamped {}), covered {:?}: expected {expected:?}",
        witness.serial,
        witness.offset,
        witness.visible,
        witness.count_requested,
        witness.count_clamped,
        covered.iter().map(|r| r.sequence).collect::<Vec<_>>()
    );
    if expected == *l1 {
        H5::Supported {
            untruncated: end == witness.visible
                && (witness.returned as u64) < u64::from(witness.count_requested),
        }
    } else {
        H5::Refuted
    }
}

/// The content store's release of the lane's resource, joined to its
/// admission: the transaction is the upload's offer id, the grant the
/// attempt's, the resource the lane's, the reason None (not Revoked).
fn released_record(
    credit: &OwnedCreditFacts,
    grant: ContentGrant,
) -> (TransactionId, ShellContentRecord) {
    (
        TransactionId::from_raw(credit.offer.request.slot.offer),
        ShellContentRecord::ResourceReleased(ContentResourceReleased {
            grant,
            resource: RESOURCE,
            reason: ContentReason::None as u16,
        }),
    )
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
    let job = rendered(&mut lane, &wake, &mut a);
    let (key, n) = lane.admitted_key(job);
    let (pp0, _) = peer_poll(&mut a);
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
    let status = one_status(&w1, a.key.grant, "R4 T1");
    let settled = status_of(status).unwrap();
    assert_eq!(
        (settled.grant, settled.resource, settled.status),
        (a.key.grant, RESOURCE, 2),
        "the identified acceptance"
    );
    assert!(
        peer_terminals(&frames_in(&w1), key, n).is_empty(),
        "no terminal"
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
    let (pp1, l1) = peer_poll(&mut a);
    assert!(
        pp1.handled < status.sequence,
        "unread: handled {} before status {}",
        pp1.handled,
        status.sequence
    );
    assert!(l1.is_empty(), "nothing readable: {l1:?}");
    assert!(peer_terminals(&a.frames, key, n).is_empty());
    assert!(
        peer_statuses(&a.contents, a.key.grant).is_empty(),
        "the peer has not read the acceptance"
    );
    let t2 = lane.result_turn();
    assert_eq!(t2.steps, TURN);
    // `accepted` is the fixture's cumulative count at each content boundary.
    assert_eq!(t2.accepted, t1.accepted, "nothing more to accept");
    assert_eq!(t2.accepted, [1]);
    assert_eq!(lane.native.accepts(), 1, "the fixture total");
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
    let (pp2, l2) = peer_poll(&mut a);
    assert!(pp2.handled < status.sequence, "still unread");
    assert!(l2.is_empty(), "{l2:?}");
    assert_eq!(a.accepted, 0);
    assert_eq!(lane.releases(), (0, 0), "the head holds the lease");
    assert_eq!(lane.leased(), 1);
}

/// P1: the invalidation queues AllocationResult(50) in the outbox between
/// turns without a drain (flush_content_allocation_events); the journal is
/// unchanged. T1 commits it, the resource's refusal status and the
/// candidate's one AllocationLost terminal; the stream's Closed record for
/// the lost allocation is the predicted fourth, under the exact admitted
/// key. The result commits before the status; the status/terminal order
/// is observed. Nothing is deferred at the tail: the terminal ended the
/// frame's flight. Then the schedule: PP1 reads what T1's one reply
/// returned, IT serves the re-armed read, PP2 reads the rest.
#[test]
fn p1_a_lost_allocation_journals_its_result_the_refusal_and_the_terminal_in_t1() {
    let (mut lane, wake) = Lane::running();
    let mut a = lane.attempt();
    let job = rendered(&mut lane, &wake, &mut a);
    let (key, n) = lane.admitted_key(job);
    assert_eq!(
        (key.grant, key.output, key.allocation),
        (a.key.grant, OUTPUT, ALLOCATION)
    );
    let (pp0, _) = peer_poll(&mut a);
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
    let status = one_status(&w1, a.key.grant, "P1 T1");
    let refusal = status_of(status).unwrap();
    assert_ne!(refusal.status, 2, "a refusal: {refusal:?}");
    assert_eq!(
        peer_terminals(&frames_in(&w1), key, n),
        [LOST],
        "one AllocationLost terminal"
    );
    let terminal = w1
        .iter()
        .find(|r| frame_of(r).is_some_and(|f| outcome_for(f, key, n).is_some()))
        .unwrap();
    let closed = peer_closed(&frames_in(&w1), key);
    assert_eq!(
        closed.len(),
        1,
        "the stream closed under its exact key: {:?}",
        kinds(&w1)
    );
    assert_eq!(
        closed[0].1,
        ContentReason::AllocationLost as u16,
        "closed with its allocation"
    );
    assert!(
        result.sequence < status.sequence,
        "the result {} commits before the status {}",
        result.sequence,
        status.sequence
    );
    eprintln!(
        "P1: refusal {refusal:?}; status {} {} terminal {}; Closed under {:?}",
        status.sequence,
        if status.sequence < terminal.sequence {
            "before"
        } else {
            "after"
        },
        terminal.sequence,
        closed[0].0
    );
    assert_all_visible(s1.journal(), "P1 O1");
    assert_eq!(s1.facts.lean_frames_in_flight, Some(0));
    assert!(s1.facts.outbox.is_empty(), "{:?}", s1.facts.outbox);
    assert!(!s1.facts.output_blocked);
    s1.assert_stores_empty("P1 O1");
    let (verdict, l1, l2, s3) = lane.schedule(&mut a, &s0, &s1, &[], "P1");
    verdict.assert_established("P1");
    assert_eq!(
        l1.len() + l2.len(),
        4,
        "the peer surfaced T1's four records once"
    );
    assert_eq!(
        peer_terminals(&a.frames, key, n),
        [LOST],
        "the terminal once"
    );
    assert_eq!(
        peer_statuses(&a.contents, a.key.grant),
        [(refusal.status, refusal.reason)],
        "the refusal once"
    );
    assert_eq!((a.accepted, a.refused), (0, 1));
    assert_eq!(peer_closed(&a.frames, key), closed, "the Closed once");
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
/// Submitted (a candidate-class request, joined to the peer's close ticket
/// through its custody), Closed under the Close's transaction with the
/// exact key, and the acceptance; no terminal for candidate 1. The
/// status/Closed order is observed and the marker follows it: a status
/// committed in flight is deferred and the ordinary Closed leaves its
/// marker stale; a status after Closed publishes at once. Either way Closed
/// zeroed the lean count, all is visible and the peer gains Closed and the
/// acceptance. The still-submitted head and its pending presentation are
/// separate facts, unchanged by the close.
#[test]
fn p2_a_close_after_admission_journals_closed_under_its_transaction_and_no_terminal() {
    let (mut lane, wake) = Lane::running();
    let mut a = lane.attempt();
    let job = rendered(&mut lane, &wake, &mut a);
    let binding = lane.h.owner.processes.combined_row(0).binding(job).unwrap();
    let (key, n) = lane.admitted_key(job);
    let admission = a.stream.close(&mut a.peer, CLOSE).unwrap();
    assert_eq!(admission.count, 1, "one Close record");
    let (pp0, _) = peer_poll(&mut a);
    let s0 = lane.snapshot(&a, "P2 O0");
    let custody0 = a.peer.custody(admission.first);
    eprintln!(
        "P2 PP0 {pp0:?}; O0 journal {:?}; close custody {custody0:?}",
        s0.journal()
    );
    assert_ne!(
        custody0,
        Some(Custody::Submitted),
        "not yet admitted by the owner"
    );
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
        peer_terminals(&frames_in(&w1), key, n).is_empty(),
        "no terminal for the admitted frame"
    );
    let submitted: Vec<ShellFileSubmitted> = w1.iter().filter_map(submitted_of).collect();
    assert_eq!(submitted.len(), 1, "one Submitted: {:?}", kinds(&w1));
    let submitted = submitted[0];
    let submission_id = submitted.submission_id;
    assert_eq!(
        submitted.candidate_kind,
        ShellFileKind::FrameStreamClose,
        "the Close's own Submitted"
    );
    assert_ne!(submission_id, 0, "a peer submission id");
    assert!(
        !s0.submission_ids().contains(&submission_id),
        "a new submission id, not an earlier record's"
    );
    let closed = peer_closed(&frames_in(&w1), key);
    assert_eq!(
        closed.len(),
        1,
        "one Closed under the exact key: {:?}",
        kinds(&w1)
    );
    assert_eq!(closed[0].0, CLOSE, "under the Close's transaction");
    eprintln!("P2: Closed reason {}", closed[0].1);
    let closed_record = w1
        .iter()
        .find(|r| frame_of(r).is_some_and(|f| closed_for(f, key).is_some()))
        .unwrap();
    let status = one_status(&w1, a.key.grant, "P2 T1");
    let settled = status_of(status).unwrap();
    assert_eq!(
        (settled.grant, settled.resource, settled.status),
        (a.key.grant, RESOURCE, 2),
        "the frame's acceptance"
    );
    let status_first = status.sequence < closed_record.sequence;
    eprintln!(
        "P2: status {} {} Closed {}",
        status.sequence,
        if status_first { "before" } else { "after" },
        closed_record.sequence
    );
    let j1 = s1.journal();
    assert_eq!(
        s1.facts.lean_frames_in_flight,
        Some(0),
        "Closed ended every flight (export: FrameStreamClosed zeroes the count)"
    );
    assert_all_visible(j1, "P2 O1");
    assert_eq!(
        j1.deferred,
        if status_first {
            Some((status.start, status.end))
        } else {
            None
        },
        "the marker: stale behind the ordinary Closed, or cleared by a status published at once"
    );
    assert!(s1.facts.outbox.is_empty(), "{:?}", s1.facts.outbox);
    s1.assert_stores_empty("P2 O1");
    let (verdict, l1, l2, _) = lane.schedule(&mut a, &s0, &s1, &[], "P2");
    verdict.assert_established("P2");
    assert_eq!(
        l1.len() + l2.len(),
        2,
        "the peer surfaced Closed and the acceptance once (Submitted is consumed by the client)"
    );
    assert_eq!(
        peer_closed(&a.frames, key),
        [(CLOSE, closed[0].1)],
        "the peer read Closed once"
    );
    assert_eq!(
        a.peer.custody(admission.first),
        Some(Custody::Submitted),
        "the peer's Close ticket observed its Submitted"
    );
    assert!(
        peer_terminals(&a.frames, key, n).is_empty(),
        "no terminal reached the peer"
    );
    assert_eq!(
        peer_statuses(&a.contents, a.key.grant),
        [(2, settled.reason)],
        "the acceptance reached the peer"
    );
    assert_eq!((a.accepted, a.refused), (1, 0));
    assert!(!a.stream.has_credit(), "{:?}", a.stream.state());
    assert_eq!(
        lane.pending(&a),
        [(a.key.grant, OUTPUT, 1)],
        "the head's presentation is still pending"
    );
    assert_eq!(lane.releases(), (0, 0), "the head holds the lease");
    assert_eq!(lane.leased(), 1);
}

/// P3: the candidate's own expiry. T1's content service drains the
/// acceptance first (deferred, since the frame is in flight), then its
/// expiry commits the Timeout terminal by an ordinary append: the marker
/// stays stored but stale, both records are visible after T1 and the
/// terminal's own commit answers the peer's parked read, so one peer-only
/// poll reads both (the accepted row's `peer_reads_terminal` needed no
/// owner call). Compared with P1/P2, nothing precedes the terminal that
/// could answer the read early.
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
    let (key, n) = lane.admitted_key(job);
    let deadline = lane.deadline_of(job);
    let (pp0, _) = peer_poll(&mut a);
    let s0 = lane.snapshot(&a, "P3 O0");
    eprintln!("P3 PP0 {pp0:?}; O0 journal {:?}", s0.journal());
    let credit = resource_credit(&s0, "P3 O0");
    let release = released_record(&credit, a.key.grant);
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
    let status = one_status(&w1, a.key.grant, "P3 T1");
    assert_eq!(peer_terminals(&frames_in(&w1), key, n), [TIMEOUT]);
    let j1 = s1.journal();
    assert_all_visible(j1, "P3 O1");
    assert_eq!(
        j1.deferred,
        Some((status.start, status.end)),
        "the deferred acceptance's marker is stored but stale behind the ordinary terminal"
    );
    assert_eq!(s1.facts.lean_frames_in_flight, Some(0));
    assert!(s1.facts.outbox.is_empty(), "{:?}", s1.facts.outbox);
    // The Timeout abandons the frame image and the pixels are exclusive, so
    // T1's collect queues the store's release after the pass's drain. It is
    // pending here, journaled by IT and read by PP2; T1's own reply and PP1
    // carry the status and terminal alone.
    assert_eq!(
        s1.stores.resources,
        [release.clone()],
        "P3 O1: exactly the identified release is pending"
    );
    assert!(
        s1.stores.candidates.is_empty() && s1.stores.allocations.is_empty(),
        "P3 O1: {:?}",
        s1.stores
    );
    let (verdict, l1, _, s3) = lane.schedule(&mut a, &s0, &s1, &[release], "P3");
    assert_eq!(
        (l1.contents.len(), l1.frames.len()),
        (1, 1),
        "one peer-only poll reads both: {l1:?}"
    );
    assert_eq!(
        verdict,
        H5::Supported { untruncated: true },
        "T1's one reply returned both records"
    );
    // The queue drained into the journal; nothing here shows the backing
    // destroyed (that follows the commit in confirm_isb_release, unobserved).
    s3.assert_stores_empty("P3 O3");
    assert_eq!(
        peer_terminals(&a.frames, key, n),
        [TIMEOUT],
        "the expiry, once"
    );
    assert_eq!(
        peer_statuses(&a.contents, a.key.grant),
        [(2, status_of(status).unwrap().reason)],
        "the one status; the release is a content record, not a status"
    );
    assert_eq!((a.accepted, a.refused), (1, 0));
    assert_eq!(lane.candidate_of(&a, binding, job), Some(Unknown));
    assert_eq!(
        lane.states(),
        [(job, "held")],
        "Held kept before the deadline"
    );
    assert_eq!(lane.releases(), (0, 0));
}

/// The fault rows F1-F4, split for the file-size limit.
#[path = "combined_publication_faults.rs"]
mod faults;
