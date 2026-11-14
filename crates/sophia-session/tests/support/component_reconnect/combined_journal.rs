//! G3-5d d-2b3d-2 Session controls over the parent's lane, observers,
//! windows and peer polls: the full journal under a withheld ACK (FJ, with
//! MD7's full-journal consumer), the next admission's Stop at the
//! reservation refusal (FJ-stop), the declared completion of F4's queued
//! fallback (F4-O, a simulated completion) and the nonempty outbox cleared
//! by disconnect (BP). Design REV3 D3/D5 and REV4 D3/D4-O, the accounting
//! REV6, the fill REV7 (section 2: the baseline ACK) and its amendments
//! A2 (REV9: the images-step order) and A3 (REV10: the prefix).
//!
//! Journal capacity (files/journal.rs `limits`, `append_limits`,
//! `hold_reservation`): credited records fit while `records < 256 − held`,
//! uncredited while `records < 192`; a reservation is taken under the
//! credited rule. `held` is a total. One reservation is persistent in this
//! lane, the channel-close record held since ChannelReady (export/image.rs),
//! so the baseline is H = 1 and an admitted offer makes it 3: the status
//! hold (spent by the ResourceStatus in T1) and the release hold (spent by
//! SlotReleased). `bytes` and `held` are printed at every boundary rather
//! than derived. R below is the count of retained records since the
//! baseline ACK A_B, an occupancy; every ACK is joined to an observed
//! header sequence, never to R.
//!
//! Producers (REV10 A3, every count a prediction until this runs). After
//! A_B the peer reads but never ACKs, so only lean frames accumulate: a
//! non-lean request journals a Submitted the client must see acknowledged
//! before its next record (vendored submission.rs `start_next_outbound`).
//! A lean C3 filler F journals FOUR records (validation-03 F3): the status
//! (reserved) and FrameOutcome(3, 9 RendererFailed) (credited) in T1;
//! after slot freedom SlotReleased (reserved, T2 images) and
//! ResourceReleased (credited, T2 content: `collect` after the lease
//! release, flushed in the Resources stage; `confirm_isb_release` only
//! then releases the backing charge). Holds 1 → 3 → 2 → 1. The prefix
//! frame I is a lean frame whose pixels fail the premultiplied scan (the
//! returned-plan control's INVALID): the worker consumes the ring read
//! before the result exists, the combined route returns the plan and the
//! Err travels the ordinary validator route (no renderer forward, no
//! lease, no render); the owner's reconcile aborts the transfer
//! (staging, reserved-resident and backing decharged, a refusal status
//! queued, no Accepted entry, so never a ResourceReleased) and
//! `reject_assembly` queues FrameOutcome(3, 3 Malformed) carrying the
//! stream credit back; the owned release waits (EAGAIN) while the status
//! is queued and publishes SlotReleased(3) in the next images step. THREE
//! visible records, holds back to H, nothing leased or acquired. Two I
//! frames (R 6) and 61 fillers reach 250, the one start from which a
//! filler's T2 both commits its SlotReleased (253) and admits the next
//! frame (held 3) before the Resources stage flushes its ResourceReleased,
//! refused at 253 == 256 − 3 (owned_copy.rs `service_with`: reconcile,
//! publish_releases, propose_offer, propose_frame_commit, then content).
//!
//! FJ: frames 1, 2 = I; 3..=63 = F (R 250); 64 = F whose T2 is the one
//! designated turn in which frame 65, submitted after 64's terminal
//! returned the credit, must be admitted (a miss is a gap, never a retry);
//! ResourceReleased(64) parks, `output_blocked`. T1(65): C3; its status
//! and outcome queue behind the parked release (three entries). T2(65),
//! the full driver turn after slot freedom: the status guard refuses
//! SlotReleased(65) (MD7's site) and the Resources stage parks
//! ResourceReleased(65) fourth. The ACK is derived from the peer's
//! observed handled sequence: through SlotReleased(64) (published, lean 0
//! at its commit) R 0, after the four-record drain 4, after T3's
//! SlotReleased(65) 5; through outcome(64) 1, 5, 6. No other frontier is
//! accepted. ResourceReleased(65) precedes SlotReleased(65) on this
//! blocked path, the reverse of a completed filler. MD7 (delete the
//! `credit.status.is_some()` guard in owned_ring/release.rs) is caught by
//! the parent R4's order assertion first; here its full-journal consumer
//! is the blocked frame's publication counter, which must still equal the
//! pre-block baseline (SlotReleased(64)) at O1 and O2 while the status is
//! parked. Earlier prefix, staging, timing or accounting failures are not
//! detections.
//!
//! FJ-stop (settled scope): 63 fillers (R 252); the final filler 64 ends
//! with status 253, outcome 254, SlotReleased(64) published at 255 and
//! ResourceReleased(64) parked (`output_blocked`, backing charged). The
//! probe 65 is refused at its first reservation hold (255 not < 256 − 1):
//! `Step::Stop`, the offer waits, nothing is appended or queued, holds
//! stay H. The other blockers are shown absent from the admission facts
//! (worker capacity, the backing budget with the parked charge, control
//! room), the outbox and the stores; the first-hold attribution is an
//! inference from those facts and the source order. The ACK, then the
//! drain of the one parked release, then admission.
//!
//! Timing: `check_ack_progress` closes a blocked reader 2 s after the
//! journal's `last_progress`, which only an ACK advances. Before EVERY
//! input-serving pass after A_B, blocked or not, including each pass of
//! the journal-local admission and settlement loops, the budget 2 s −
//! 200 ms from the baseline is asserted; a miss is the named failure,
//! never a retry. Every driver turn here is the inherited full turn
//! (images with the owner pass, content, walk, native service); nothing
//! pauses native service. A fresh wake boundary precedes every submission
//! while the previous frame's result is proven collected and its slot
//! free; I rings no renderer wake and is never waited on it.
//!
//! Backing destruction is not observed; a journal commit is neither peer
//! consumption nor a renderer acknowledgement; the renderer lease is a
//! third domain. Off/native-session remain unpassed and unwaived.
//!
//! Client offer records (source-04, the validation-01 diagnosis): the
//! producer's four offer records are freed only by `image_forget`, so a
//! filler's offer is forgotten once its exact release was read and
//! applied (`combined_journal_offers.rs`); the INVALID prefix's refusal
//! release is read in the following frame's polls and forgotten there.
//! Every admission boundary prints its raw facts before the frozen
//! assertions.
use super::*;
use offers::*;
use sophia_protocol::shell_files::{
    SHELL_FILE_ACK_PROGRESS_TIMEOUT_MILLIS, SHELL_FILE_MAX_JOURNAL_RECORDS,
    SHELL_FILE_TERMINAL_RESERVE_RECORDS,
};
use sophia_runtime::{ContentMemoryUsage, OutboxEntryFacts};
use sophia_shell_client::RingOfferState;

/// The protocol's journal record limit and its uncredited limit, asserted
/// against the constants in phase 0.
const MAX_RECORDS: usize = 256;
const UNCREDITED_LIMIT: usize = 192;
/// The persistent channel-close reservation: total holds between frames.
const H: usize = 1;
const PRESENTED: (u16, u16) = (2, ContentReason::None as u16);
/// FJ: the prefix, the fillers to 250, the filler at 250, the blocked frame.
const PREFIX: u64 = 2;
const FJ_FILLERS: u64 = 63;
const FJ_LAST: u64 = 64;
const FJ_BLOCK: u64 = 65;
/// FJ-stop: the fillers to 252, the final filler at 252, the probe.
const STOP_FILLERS: u64 = 63;
const STOP_FINAL: u64 = 64;
const STOP_PROBE: u64 = 65;

/// The fill's fixed facts from phase 0.
struct Fill {
    /// The journal's `last_progress` at the baseline: the budget's origin.
    progress_0: Instant,
    /// The peer's queued ACK at the baseline, unchanged until the unblock.
    queued_0: u64,
    /// The backing charge with nothing admitted.
    backing_0: u64,
    /// The ring offers submitted and not yet forgotten.
    offers: Vec<LiveOffer>,
}

/// The ACK-progress budget from the baseline, before an input-serving pass.
fn budget(fill: &Fill, phase: &str) {
    let budget = Duration::from_millis(u64::from(SHELL_FILE_ACK_PROGRESS_TIMEOUT_MILLIS) - 200);
    let elapsed = fill.progress_0.elapsed();
    assert!(
        elapsed < budget,
        "ACK-progress budget exceeded at {phase}: {elapsed:?} of {budget:?}"
    );
}

fn resource_n(n: u64) -> ContentResourceId {
    ContentResourceId {
        id: RESOURCE.id,
        generation: n,
    }
}

fn usage(lane: &Lane) -> ContentMemoryUsage {
    lane.h
        .owner
        .processes
        .content_usage(0)
        .expect("the store grant has an active epoch")
}

/// The fixture's absolute counters: accepts, renderer acquisitions and
/// (release attempts, Transferred receipts). The content-boundary
/// observation of a turn is absolute too, so a frame's expectation is the
/// prior count plus exactly its own change (source-01 R2).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Counters {
    accepts: usize,
    acquisitions: usize,
    releases: (usize, usize),
}

fn counters(lane: &Lane) -> Counters {
    Counters {
        accepts: lane.native.accepts(),
        acquisitions: lane.acquisitions(),
        releases: lane.releases(),
    }
}

impl Counters {
    /// The content boundary of a turn in which the arm neither accepted
    /// nor released.
    fn unchanged(self) -> [Observed; 1] {
        [(self.accepts, self.releases)]
    }

    /// After exactly one release, answered Transferred, before the walk.
    fn released(self) -> Self {
        Self {
            releases: (self.releases.0 + 1, self.releases.1 + 1),
            ..self
        }
    }

    fn after_render(self) -> Self {
        Self {
            acquisitions: self.acquisitions + 1,
            ..self
        }
    }
}

/// The live credit for `resource`.
fn credit_of(s: &Snapshot, resource: ContentResourceId, label: &str) -> OwnedCreditFacts {
    let credits: Vec<OwnedCreditFacts> = s
        .facts
        .owned_live_credits
        .iter()
        .copied()
        .filter(|credit| credit.offer.request.resource == resource)
        .collect();
    assert_eq!(
        credits.len(),
        1,
        "{label}: one live credit for {resource:?} in {:?}",
        s.facts.owned_live_credits
    );
    credits[0]
}

/// The store's release of `resource`, joined to its admission (the offer
/// id), with reason None.
fn release_of(
    credit: &OwnedCreditFacts,
    grant: ContentGrant,
    resource: ContentResourceId,
) -> (TransactionId, ShellContentRecord) {
    (
        TransactionId::from_raw(credit.offer.request.slot.offer),
        ShellContentRecord::ResourceReleased(ContentResourceReleased {
            grant,
            resource,
            reason: ContentReason::None as u16,
        }),
    )
}

fn release_identity(release: &(TransactionId, ShellContentRecord)) -> RecordIdentity {
    RecordIdentity::Content {
        transaction: release.0,
        record: release.1.clone(),
    }
}

/// The window's one ResourceStatus under `grant` for `resource`.
fn status_for<'a>(
    records: &'a [RetainedRecord],
    grant: ContentGrant,
    resource: ContentResourceId,
    label: &str,
) -> &'a RetainedRecord {
    let statuses: Vec<&RetainedRecord> = records
        .iter()
        .filter(|record| {
            status_of(record)
                .is_some_and(|status| status.grant == grant && status.resource == resource)
        })
        .collect();
    assert_eq!(
        statuses.len(),
        1,
        "{label}: one ResourceStatus for {resource:?} in {:?}",
        kinds(records)
    );
    statuses[0]
}

/// The (status, reason) of every status the peer surfaced for `resource`.
fn statuses_for(
    contents: &[(TransactionId, ShellContentRecord)],
    grant: ContentGrant,
    resource: ContentResourceId,
) -> Vec<(u16, u16)> {
    contents
        .iter()
        .filter_map(|(_, record)| match record {
            ShellContentRecord::ResourceStatus(status)
                if status.grant == grant && status.resource == resource =>
            {
                Some((status.status, status.reason))
            }
            _ => None,
        })
        .collect()
}

/// The store's resource events at a boundary where the worker may or may
/// not have released the lease yet (REV6 B4): empty, or exactly `release`;
/// candidates and allocations empty. Whether it is pending.
fn store_release(s: &Snapshot, release: &(TransactionId, ShellContentRecord), label: &str) -> bool {
    assert!(
        s.stores.resources.is_empty() || s.stores.resources == std::slice::from_ref(release),
        "{label}: empty or exactly the frame's release: {:?}",
        s.stores.resources
    );
    assert!(
        s.stores.candidates.is_empty() && s.stores.allocations.is_empty(),
        "{label}: {:?}",
        s.stores
    );
    !s.stores.resources.is_empty()
}

/// Short labels for the outbox FIFO, as `kinds` for a window.
fn outbox_kinds(outbox: &[OutboxEntryFacts]) -> Vec<String> {
    outbox
        .iter()
        .map(|entry| match &entry.record {
            OutboxRecordFacts::Content(_, ShellContentRecord::AllocationResult(_)) => {
                "AllocationResult".into()
            }
            OutboxRecordFacts::Content(_, ShellContentRecord::ResourceStatus(status)) => {
                format!("ResourceStatus({})", status.status)
            }
            OutboxRecordFacts::Content(_, ShellContentRecord::ResourceReleased(_)) => {
                "ResourceReleased".into()
            }
            OutboxRecordFacts::Content(_, record) => format!("Content({record:?})"),
            OutboxRecordFacts::Frame(ShellFrameRecord::Outcome { outcome, .. }) => {
                format!("FrameOutcome({},{})", outcome.kind, outcome.reason)
            }
            OutboxRecordFacts::Frame(frame) => format!("Frame({:?})", frame.kind()),
            OutboxRecordFacts::Other(name) => format!("Other({name})"),
        })
        .collect()
}

/// The exact parked release: the outbox entry `at` is `release`.
fn assert_parked(
    s: &Snapshot,
    at: usize,
    release: &(TransactionId, ShellContentRecord),
    label: &str,
) {
    assert_eq!(
        s.facts.outbox.get(at).map(|entry| &entry.record),
        Some(&OutboxRecordFacts::Content(release.0, release.1.clone())),
        "{label}: the exact release parked at {at}: {:?}",
        outbox_kinds(&s.facts.outbox)
    );
}

/// The first retained record's sequence: the retention floor's identity.
fn first_retained(s: &Snapshot) -> Option<u64> {
    s.facts.table.first().and_then(|entry| match entry {
        RetainedEntry::Record(record) => Some(record.sequence),
        _ => None,
    })
}

/// Phase 0, after `attempt`: the ACK baseline with automatic ACKs on (one
/// peer poll and one IT apply any ACK already queued), then automatic ACKs
/// off. Asserts the protocol limits, the persistent hold and R_B = 0.
fn baseline(lane: &mut Lane, a: &mut Attempt, label: &str) -> Fill {
    assert_eq!(MAX_RECORDS, usize::from(SHELL_FILE_MAX_JOURNAL_RECORDS));
    assert_eq!(
        UNCREDITED_LIMIT,
        MAX_RECORDS - usize::from(SHELL_FILE_TERMINAL_RESERVE_RECORDS)
    );
    // A_B (REV7 section 2). 1: the peer has processed every committed
    // record and no ACK is in flight. The pass before it drains a lone
    // automatic lean ACK still in flight.
    let p0 = a.peer.journal_progress();
    a.peer.poll_io().unwrap();
    observe_peer(a);
    lane.input_turn(a);
    let (p1, _) = peer_poll(a);
    let s_pre = lane.snapshot(a, label);
    let j_pre = s_pre.journal();
    assert_eq!(
        p1.handled,
        j_pre.next_sequence - 1,
        "{label}: the peer processed every committed record: {p1:?} {j_pre:?}"
    );
    assert_eq!(j_pre.deferred, None, "{label}: nothing deferred: {j_pre:?}");
    assert!(!p1.ack_pending, "{label}: no ACK in flight: {p1:?}");
    // 2: manual from here; retracts nothing.
    a.peer.set_automatic_journal_ack(false).unwrap();
    // 3: the covering ACK, bound = handled; sent by the peer's poll, applied
    // by one owner input-serving turn, confirmed by the peer's next poll. A
    // false return means the automatic ACK already covered handled.
    let sent = a.peer.acknowledge_journal().unwrap();
    a.peer.poll_io().unwrap();
    lane.input_turn(a);
    let (pp, _) = peer_poll(a);
    assert!(
        pp.acknowledged == pp.queued && pp.queued == p1.handled && !pp.ack_pending,
        "{label}: A_B covers handled {} (sent {sent}): {pp:?}",
        p1.handled
    );
    // 4: the released prefix is everything committed.
    let s0 = lane.snapshot(a, label);
    let j = s0.journal();
    assert_eq!(
        j.held, H,
        "{label}: the persistent channel-close hold alone: {j:?}"
    );
    assert_eq!(j.records, 0, "{label}: R_B: everything acknowledged: {j:?}");
    assert_eq!(j.floor, j.tail, "{label}: {j:?}");
    assert!(!s0.facts.output_blocked);
    let usage_0 = usage(lane);
    assert!(
        a.stream.has_credit(),
        "{label}: the stream holds its credit"
    );
    // 5: the bound; `queued` must stay equal to it until the deliberate
    // unblocking ACK.
    eprintln!(
        "{label}: A_B {p0:?} -> {p1:?} -> {pp:?} (sent {sent}); journal {j:?}; usage {usage_0:?}; counters {:?}",
        counters(lane)
    );
    Fill {
        progress_0: j.last_progress,
        queued_0: pp.queued,
        backing_0: usage_0.backing,
        offers: Vec::new(),
    }
}

/// R and held, read between calls.
fn records(lane: &Lane, a: &Attempt, label: &str) -> (usize, usize) {
    let s = lane.snapshot(a, label);
    let j = s.journal();
    (j.records, j.held)
}

/// A fresh notification boundary before a submission (source-01 R4):
/// the previous frame's result was collected (no job in flight) and its
/// renderer slot is free, so the wake's possible ring is that result's,
/// already observed; it is cleared, and the next ring is the next
/// result's. Never called after an admission.
fn boundary(lane: &Lane, wake: &sophia_wake::Wake, a: &Attempt, label: &str) {
    assert!(
        !lane.states().iter().any(|(_, state)| *state == "in flight"),
        "{label}: a job is still in flight: {:?}",
        lane.states()
    );
    assert_eq!(lane.leased(), 0, "{label}: the previous slot is free");
    assert!(a.stream.has_credit(), "{label}: the stream credit is back");
    wake.clear().unwrap();
    assert!(
        !wake.wait(Some(Instant::now())).unwrap(),
        "{label}: the wake is empty before the submission"
    );
}

/// Driver turns with `a`'s peer polled until the row holds exactly one
/// job, in flight, with the budget checked before each pass (`admitted`'s
/// criteria and call order; source-01 R5). The job.
fn admit(lane: &mut Lane, a: &mut Attempt, fill: &Fill, label: &str) -> u64 {
    let deadline = Instant::now() + WAIT;
    let mut passes = 0;
    loop {
        budget(fill, &format!("{label} admission pass {passes}"));
        a.peer.poll_io().unwrap();
        assert_eq!(lane.quiet_driver_turn(true), TURN);
        observe_peer(a);
        passes += 1;
        if let [(job, "in flight")] = lane.states().as_slice() {
            eprintln!("{label}: admitted after {passes} pass(es)");
            return *job;
        }
        assert!(
            Instant::now() < deadline,
            "{label}: no admission: {:?}",
            lane.states()
        );
        std::thread::yield_now();
    }
}

/// Prefix frame `n` (I): a lean frame of INVALID pixels through the running
/// validator to its Malformed terminal and the published SlotReleased(3),
/// with no renderer involvement. Three records; R after.
fn invalid_frame(
    lane: &mut Lane,
    wake: &sophia_wake::Wake,
    a: &mut Attempt,
    fill: &mut Fill,
    n: u64,
    label: &str,
) -> usize {
    boundary(lane, wake, a, label);
    let c0 = counters(lane);
    let s_pre = lane.snapshot(a, &format!("{label} pre"));
    let (r0, held0) = (s_pre.journal().records, s_pre.journal().held);
    assert_eq!(held0, H, "{label}: the persistent hold alone");
    assert_eq!(
        usage(lane).backing,
        fill.backing_0,
        "{label}: nothing charged"
    );
    budget(fill, label);
    client_facts(a, fill, label);
    let offer = lane.submit_at(a, n, INVALID, &[0]);
    offered(fill, a, n, offer, RingOfferState::Refused, label);
    let job = admit(lane, a, fill, label);
    let (key, generation) = lane.admitted_key(job);
    assert_eq!(generation, n);
    let resource = resource_n(n);
    let s0 = lane.snapshot(a, &format!("{label} O0"));
    let j0 = s0.journal();
    admission_facts(lane, a, &s_pre, &s0, &format!("{label} O0"));
    assert_eq!(j0.records, r0, "{label} O0: the admission journals nothing");
    assert_eq!(j0.held, H + 2, "{label} O0: the offer's two holds: {j0:?}");
    let credit = credit_of(&s0, resource, &format!("{label} O0"));
    assert!(credit.status_reserved);
    let identity = credit.offer.identity;
    assert!(s0.releasing.is_empty());
    assert!(
        usage(lane).backing > fill.backing_0,
        "{label} O0: the admission charged the backing"
    );
    assert_eq!(s0.facts.lean_frames_in_flight, Some(1));
    // Full driver turns until the validator's completion is reconciled:
    // passes before the result commit nothing and leave the job in flight;
    // the result-bearing T1 commits the refusal and the terminal.
    let deadline = Instant::now() + WAIT;
    let mut pre = 0;
    let mut prev = s0;
    let (s1, w1) = loop {
        budget(fill, &format!("{label} pass after {pre} empty"));
        a.peer.poll_io().unwrap();
        let (steps, observed) = lane.observed_turn(Instant::now(), None);
        assert_eq!(steps, TURN);
        assert_eq!(observed, c0.unchanged(), "{label}: no accept, no release");
        let s = lane.snapshot(a, &format!("{label} after a pass"));
        let w = window(&prev, &s, &format!("{label} pass"));
        if !w.is_empty() {
            break (s, w);
        }
        assert_eq!(
            lane.states(),
            [(job, "in flight")],
            "{label}: a pre-result pass leaves the job in flight"
        );
        assert_eq!(s.facts.outbox, prev.facts.outbox);
        pre += 1;
        prev = s;
        assert!(
            Instant::now() < deadline,
            "{label}: the validation result never reached the owner"
        );
        std::thread::yield_now();
    };
    assert_eq!(
        kinds(&w1),
        ["ResourceStatus(3)", "FrameOutcome(3,3)"],
        "{label} T1: the refusal, then the Malformed terminal"
    );
    let status = status_for(&w1, a.key.grant, resource, &format!("{label} T1"));
    assert_eq!(
        status_of(status).unwrap().reason,
        ContentReason::Malformed as u16
    );
    assert_eq!(peer_terminals(&frames_in(&w1), key, n), [MALFORMED]);
    let j1 = s1.journal();
    assert_eq!(
        j1.held,
        H + 1,
        "{label} O1: the status hold is spent: {j1:?}"
    );
    assert_eq!(s1.releasing, [identity], "{label} O1: the release owed");
    assert_eq!(
        credit_of(&s1, resource, &format!("{label} O1")),
        OwnedCreditFacts {
            offer: credit.offer,
            status_reserved: false,
        }
    );
    assert_eq!(s1.facts.owned_publication_sequence, Some(status.sequence));
    assert!(s1.facts.outbox.is_empty(), "{:?}", s1.facts.outbox);
    assert!(!s1.facts.output_blocked);
    assert_all_visible(j1, &format!("{label} O1"));
    s1.assert_stores_empty(&format!("{label} O1"));
    assert_eq!(s1.facts.lean_frames_in_flight, Some(0));
    assert_eq!(
        usage(lane).backing,
        fill.backing_0,
        "{label} O1: the abort decharged the transfer"
    );
    assert_eq!(counters(lane), c0, "{label} O1: no render, no release");
    eprintln!(
        "{label} T1 after {pre} pre-result pass(es): row {:?}, pending {}, unconfirmed cancels {}, journal {j1:?}",
        lane.states(),
        lane.row().pending(),
        lane.row().unconfirmed_cancels()
    );
    // T2: the source release, published (reason 3).
    budget(fill, &format!("{label} T2"));
    a.peer.poll_io().unwrap();
    let (steps, observed) = lane.observed_turn(Instant::now(), None);
    assert_eq!(steps, TURN);
    assert_eq!(observed, c0.unchanged(), "{label} T2: nothing");
    let s2 = lane.snapshot(a, &format!("{label} O2"));
    let w2 = window(&s1, &s2, &format!("{label} T2"));
    assert_eq!(
        kinds(&w2),
        ["SlotReleased(3)"],
        "{label} T2: the failed copy's slot"
    );
    let event = slot_released_of(&w2[0]).unwrap();
    assert_eq!(
        (event.identity, event.resource, event.slot, event.reason),
        (
            identity,
            resource,
            credit.offer.request.slot,
            ContentReason::Malformed as u16
        ),
        "{label}: the original source's release, Malformed"
    );
    let j2 = s2.journal();
    assert_eq!(j2.held, H, "{label} O2: both holds spent: {j2:?}");
    assert!(s2.releasing.is_empty());
    assert!(
        !s2.facts
            .owned_live_credits
            .iter()
            .any(|c| c.offer == credit.offer),
        "{label} O2: the slot credit is spent"
    );
    assert_eq!(s2.facts.owned_publication_sequence, Some(w2[0].sequence));
    assert_all_visible(j2, &format!("{label} O2"));
    s2.assert_stores_empty(&format!("{label} O2"));
    assert!(s2.facts.outbox.is_empty());
    assert_eq!(s2.facts.lean_frames_in_flight, Some(0));
    assert!(lane.states().is_empty(), "{label} O2: the row is clear");
    assert_eq!(lane.row().pending(), 0, "{label} O2: no owed row work");
    assert_eq!(
        lane.row().unconfirmed_cancels(),
        0,
        "{label} O2: the cancel confirmed"
    );
    assert_eq!(
        lane.h.owner.processes.image_pending(0),
        0,
        "{label} O2: no image work"
    );
    assert_eq!(
        lane.residency(a, n),
        FrameImageResidency::Absent,
        "{label}: never accepted"
    );
    assert_eq!(lane.leased(), 0);
    assert_eq!(counters(lane), c0, "{label} O2: no render, no release");
    assert_eq!(usage(lane).backing, fill.backing_0);
    assert!(
        !wake.wait(Some(Instant::now())).unwrap(),
        "{label}: a validation failure rings no renderer wake"
    );
    // The peer reads the terminal: the credit is back.
    peer_reads_terminal(a, n, label);
    forget_observed(fill, a, &format!("{label} after the terminal read"));
    assert_eq!(a.terminals(n), [MALFORMED]);
    assert!(
        a.stream.has_credit(),
        "{label}: the Malformed terminal returned the credit"
    );
    assert_eq!(
        statuses_for(&a.contents, a.key.grant, resource),
        [(3, ContentReason::Malformed as u16)]
    );
    assert_eq!(
        a.peer.journal_progress().queued,
        fill.queued_0,
        "{label}: no ACK"
    );
    let own = a.peer.image_ring_offer(offer);
    assert!(
        own.is_some_and(|info| matches!(
            info.state,
            RingOfferState::Pending | RingOfferState::Refused
        )),
        "{label}: the INVALID offer after the terminal read: {own:?}"
    );
    eprintln!("{label}: offer {offer} after the terminal read: {own:?}");
    let r = j2.records;
    assert_eq!(
        r,
        r0 + 3,
        "{label}: ΔR +3: {:?}",
        kinds(&window(&prev, &s2, label))
    );
    eprintln!(
        "{label}: R {r0} -> {r}, bytes {} held {}",
        j2.bytes, j2.held
    );
    r
}

/// A filler's admitted state after its render result rang.
struct Admitted {
    job: u64,
    /// The admitted deadline plus MARGIN, read while the row holds the
    /// entry: C3 removes it, so later turns reuse this instant (source-02
    /// R1); never refreshed.
    late: Instant,
    key: ShellFrameStreamKey,
    resource: ContentResourceId,
    credit: OwnedCreditFacts,
    identity: Identity,
    release: (TransactionId, ShellContentRecord),
    /// Counters before the submission; the render adds one acquisition.
    c0: Counters,
    r0: usize,
    s0: Snapshot,
}

/// A filler's T1 facts: the status and outcome sequences and O1.
struct Declined {
    status: u64,
    outcome: u64,
    s1: Snapshot,
    /// Counters after the decline's one release.
    c1: Counters,
}

/// Frame `n` submitted at a fresh boundary, admitted, rendered (the ring),
/// its lease held, its admission's two holds taken.
fn filler_start(
    lane: &mut Lane,
    wake: &sophia_wake::Wake,
    a: &mut Attempt,
    fill: &mut Fill,
    n: u64,
    label: &str,
) -> Admitted {
    boundary(lane, wake, a, label);
    let c0 = counters(lane);
    let s_pre = lane.snapshot(a, &format!("{label} pre"));
    budget(fill, label);
    client_facts(a, fill, label);
    let offer = lane.submit_at(a, n, PIXEL, &[0]);
    offered(fill, a, n, offer, RingOfferState::Settled, label);
    let job = admit(lane, a, fill, label);
    let late = lane.deadline_of(job) + MARGIN;
    assert!(
        wake.wait(Some(Instant::now() + WAIT)).unwrap(),
        "{label}: the render result rings the owner"
    );
    assert_eq!(lane.leased(), 1, "{label}: the result holds its slot");
    assert_eq!(
        counters(lane),
        c0.after_render(),
        "{label}: one render, no release"
    );
    let (key, generation) = lane.admitted_key(job);
    assert_eq!(generation, n);
    let resource = resource_n(n);
    let s0 = lane.snapshot(a, &format!("{label} O0"));
    let j0 = s0.journal();
    admission_facts(lane, a, &s_pre, &s0, &format!("{label} O0"));
    assert_eq!(j0.held, H + 2, "{label} O0: the offer's two holds: {j0:?}");
    let credit = credit_of(&s0, resource, &format!("{label} O0"));
    assert!(credit.status_reserved);
    let identity = credit.offer.identity;
    let release = release_of(&credit, a.key.grant, resource);
    assert!(s0.releasing.is_empty());
    assert!(
        usage(lane).backing > fill.backing_0,
        "{label} O0: the admission charged the backing: {:?}",
        usage(lane)
    );
    Admitted {
        job,
        late,
        key,
        resource,
        credit,
        identity,
        release,
        c0: c0.after_render(),
        r0: j0.records,
        s0,
    }
}

/// T1 at the late instant: C3 declines at the content call; the status
/// (reserved, deferred) and the terminal (credited) commit; the peer reads
/// the terminal and holds the credit again; no ACK.
fn filler_t1(
    lane: &mut Lane,
    a: &mut Attempt,
    fill: &mut Fill,
    f: &Admitted,
    label: &str,
) -> Declined {
    budget(fill, &format!("{label} T1"));
    let (steps, observed) = lane.observed_turn(f.late, None);
    assert_eq!(steps, TURN);
    let c1 = f.c0.released();
    assert_eq!(
        observed,
        c1.unchanged(),
        "{label}: C3 declined once at the content call"
    );
    let s1 = lane.snapshot(a, &format!("{label} O1"));
    let w1 = window(&f.s0, &s1, &format!("{label} T1"));
    assert_eq!(
        kinds(&w1),
        ["ResourceStatus(2)", "FrameOutcome(3,9)"],
        "{label} T1: the acceptance, then the decline's terminal"
    );
    let status = status_for(&w1, a.key.grant, f.resource, &format!("{label} T1"));
    assert_eq!(
        peer_terminals(&frames_in(&w1), f.key, f.resource.generation),
        [FAILED]
    );
    let j1 = s1.journal();
    assert_eq!(j1.records, f.r0 + 2);
    assert_eq!(
        j1.held,
        H + 1,
        "{label} O1: the status hold is spent: {j1:?}"
    );
    assert_eq!(s1.releasing, [f.identity], "{label} O1: release owed");
    assert_eq!(
        credit_of(&s1, f.resource, &format!("{label} O1")),
        OwnedCreditFacts {
            offer: f.credit.offer,
            status_reserved: false,
        }
    );
    assert_eq!(
        s1.facts.owned_publication_sequence,
        Some(status.sequence),
        "{label} O1: the status is the ring's last publication (REV6 B3)"
    );
    assert!(s1.facts.outbox.is_empty(), "{:?}", s1.facts.outbox);
    assert!(!s1.facts.output_blocked);
    assert_all_visible(j1, &format!("{label} O1"));
    assert_eq!(s1.facts.lean_frames_in_flight, Some(0));
    let early = store_release(&s1, &f.release, &format!("{label} O1"));
    assert!(
        usage(lane).backing > fill.backing_0,
        "{label} O1: the backing stays charged until the release is journaled"
    );
    eprintln!(
        "{label} O1: leased {}, release pending in the store {early}, journal {j1:?}",
        lane.leased()
    );
    peer_reads_terminal(a, f.resource.generation, label);
    forget_observed(fill, a, &format!("{label} after T1"));
    assert_eq!(a.terminals(f.resource.generation), [FAILED]);
    assert!(
        a.stream.has_credit(),
        "{label}: the terminal returned the credit"
    );
    assert_eq!(
        a.peer.journal_progress().queued,
        fill.queued_0,
        "{label}: no ACK while withheld"
    );
    Declined {
        status: status.sequence,
        outcome: w1[1].sequence,
        s1,
        c1,
    }
}

/// A complete filler's T2 after slot freedom (SlotReleased, then
/// ResourceReleased, the backing released) and its IT; R after.
fn filler_finish(
    lane: &mut Lane,
    a: &mut Attempt,
    fill: &mut Fill,
    f: &Admitted,
    d: &Declined,
    label: &str,
) -> usize {
    lane.slot_freed(label);
    budget(fill, &format!("{label} T2"));
    a.peer.poll_io().unwrap();
    let (steps, observed) = lane.observed_turn(f.late, None);
    assert_eq!(steps, TURN);
    assert_eq!(observed, d.c1.unchanged(), "{label} T2: nothing more");
    let s2 = lane.snapshot(a, &format!("{label} O2"));
    let w2 = window(&d.s1, &s2, &format!("{label} T2"));
    assert_eq!(
        kinds(&w2),
        ["SlotReleased(0)", "ResourceReleased"],
        "{label} T2: the slot's release, then the backing's retirement"
    );
    let event = slot_released_of(&w2[0]).unwrap();
    assert_eq!(
        (event.identity, event.resource, event.slot, event.reason),
        (f.identity, f.resource, f.credit.offer.request.slot, 0),
        "{label}: the original source's release"
    );
    assert_eq!(w2[1].identity, release_identity(&f.release));
    let j2 = s2.journal();
    assert_eq!(j2.held, H, "{label} O2: both offer holds spent: {j2:?}");
    assert!(s2.releasing.is_empty());
    assert!(
        !s2.facts
            .owned_live_credits
            .iter()
            .any(|c| c.offer == f.credit.offer),
        "{label} O2: the slot credit is spent"
    );
    assert_eq!(s2.facts.owned_publication_sequence, Some(w2[0].sequence));
    assert_all_visible(j2, &format!("{label} O2"));
    s2.assert_stores_empty(&format!("{label} O2"));
    assert!(s2.facts.outbox.is_empty(), "{:?}", s2.facts.outbox);
    assert_eq!(s2.facts.lean_frames_in_flight, Some(0));
    assert_eq!(
        usage(lane).backing,
        fill.backing_0,
        "{label} O2: the backing charge is released with the journaled ResourceReleased"
    );
    budget(fill, &format!("{label} IT"));
    lane.input_turn(a);
    let s3 = lane.snapshot(a, &format!("{label} O3"));
    assert!(window(&s2, &s3, &format!("{label} IT")).is_empty());
    let (_, l) = peer_poll(a);
    assert_eq!(
        l.contents,
        std::slice::from_ref(&f.release),
        "{label}: the peer reads the release on the content queue"
    );
    assert!(l.frames.is_empty(), "{label}: {l:?}");
    forget_frame(fill, a, f.resource.generation, label);
    assert!(lane.states().is_empty(), "{label}: the row is clear");
    assert_eq!(counters(lane), d.c1, "{label}: one render, one release");
    assert_eq!(lane.native.installs(), 0);
    assert_eq!(lane.native.ordinary_queues(), 0);
    let r = s3.journal().records;
    assert_eq!(
        r,
        f.r0 + 4,
        "{label}: ΔR +4: {:?}",
        kinds(&window(&f.s0, &s3, &format!("{label} total")))
    );
    eprintln!(
        "{label}: R {} -> {r}, bytes {} held {}, status {} outcome {}",
        f.r0,
        s3.journal().bytes,
        s3.journal().held,
        d.status,
        d.outcome
    );
    r
}

/// Frame `n` through the C3 fill path (F3's accepted path) to source
/// release, backing retirement, row disposal and a free renderer slot,
/// with the four records identified and every credit transition asserted.
fn filler(
    lane: &mut Lane,
    wake: &sophia_wake::Wake,
    a: &mut Attempt,
    fill: &mut Fill,
    n: u64,
    label: &str,
) -> usize {
    let f = filler_start(lane, wake, a, fill, n, label);
    let d = filler_t1(lane, a, fill, &f, label);
    filler_finish(lane, a, fill, &f, &d, label)
}

/// Fillers `from..=to`; R after the last.
fn fillers(
    lane: &mut Lane,
    wake: &sophia_wake::Wake,
    a: &mut Attempt,
    fill: &mut Fill,
    from: u64,
    to: u64,
    label: &str,
) -> usize {
    let (mut r, _) = records(lane, a, label);
    for n in from..=to {
        r = filler(lane, wake, a, fill, n, &format!("{label} filler {n}"));
    }
    r
}

/// Which retention the peer's ACK covers, from its observed sequence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Acked {
    /// Through the published SlotReleased of the last complete filler.
    SlotReleased,
    /// Through that filler's outcome: its SlotReleased stays retained.
    Outcome,
}

/// The manual ACK (bound: the client's safe limit, its handled sequence),
/// sent by one peer poll, joined to `d`'s outcome or the later
/// `slot_released` sequence; any other frontier fails.
fn acknowledge(
    a: &mut Attempt,
    fill: &Fill,
    d: &Declined,
    slot_released: u64,
    label: &str,
) -> Acked {
    let before = a.peer.journal_progress();
    assert!(
        a.peer.acknowledge_journal().unwrap(),
        "{label}: an ACK queued through the safe limit: {before:?}"
    );
    let (pp, l) = peer_poll(a);
    assert!(
        pp.queued > fill.queued_0,
        "{label}: the ACK was sent: {pp:?}"
    );
    assert!(l.is_empty(), "{label}: nothing new to read: {l:?}");
    let acked = if pp.queued == slot_released {
        Acked::SlotReleased
    } else if pp.queued == d.outcome {
        Acked::Outcome
    } else {
        panic!(
            "{label}: ACK {} is neither outcome {} nor SlotReleased {slot_released}: {before:?} -> {pp:?}",
            pp.queued, d.outcome
        )
    };
    eprintln!(
        "{label}: ACK through {acked:?} ({}); {before:?} -> {pp:?}",
        pp.queued
    );
    acked
}

/// FJ: two prefix frames and 61 fillers to 250; the filler at 250 whose
/// one designated T2 commits its SlotReleased, admits frame 65 and parks
/// its ResourceReleased; frame 65's status and outcome queue behind it,
/// then its own release; the ring's publication counter unchanged while
/// the status is parked (MD7's full-journal consumer); the ACK derived
/// from the observed frontier, the four-record drain, then SlotReleased.
#[test]
fn fj_a_parked_release_holds_the_next_frames_records_until_an_ack() {
    let (mut lane, wake) = Lane::running();
    let mut a = lane.attempt();
    let mut fill = baseline(&mut lane, &mut a, "FJ phase 0");
    let mut r = 0;
    for n in 1..=PREFIX {
        r = invalid_frame(
            &mut lane,
            &wake,
            &mut a,
            &mut fill,
            n,
            &format!("FJ prefix {n}"),
        );
    }
    assert_eq!(r, 6, "FJ: the prefix");
    r = fillers(
        &mut lane,
        &wake,
        &mut a,
        &mut fill,
        PREFIX + 1,
        FJ_FILLERS,
        "FJ",
    );
    assert_eq!(r, 250, "FJ: the fillers reached 250");
    assert!(
        fill.offers.is_empty(),
        "FJ: every prefix and filler offer forgotten: {:?}",
        fill.offers
    );
    // Frame 64: T1 as every filler, then the staged frame 65.
    let f = filler_start(&mut lane, &wake, &mut a, &mut fill, FJ_LAST, "FJ 64");
    assert_eq!(f.r0, 250);
    let d = filler_t1(&mut lane, &mut a, &mut fill, &f, "FJ 64");
    assert_eq!(d.s1.journal().records, 252);
    lane.slot_freed("FJ 64");
    // The staged next frame: submitted on the returned credit, written to
    // the owner before the one designated turn.
    boundary(&lane, &wake, &a, "FJ 65 submission");
    budget(&fill, "FJ 65 submission");
    client_facts(&a, &fill, "FJ 65 submission");
    let offer_65 = lane.submit_at(&mut a, FJ_BLOCK, PIXEL, &[0]);
    offered(
        &mut fill,
        &a,
        FJ_BLOCK,
        offer_65,
        RingOfferState::Settled,
        "FJ 65",
    );
    assert!(!a.stream.has_credit());
    a.peer.poll_io().unwrap();
    // T2(64), the designated turn: SlotReleased(64) published at 253,
    // frame 65 admitted (held 3), ResourceReleased(64) refused and parked.
    budget(&fill, "FJ T2(64)");
    let (steps, observed) = lane.observed_turn(Instant::now(), None);
    assert_eq!(steps, TURN);
    assert_eq!(
        observed,
        d.c1.unchanged(),
        "FJ T2(64): nothing at the content call"
    );
    let s2 = lane.snapshot(&a, "FJ O2(64)");
    admission_facts(&lane, &a, &d.s1, &s2, "FJ O2(64)");
    let w2 = window(&d.s1, &s2, "FJ T2(64)");
    assert_eq!(
        kinds(&w2),
        ["SlotReleased(0)"],
        "FJ T2(64): the slot's release alone"
    );
    let event = slot_released_of(&w2[0]).unwrap();
    assert_eq!(
        (event.identity, event.resource, event.slot, event.reason),
        (f.identity, f.resource, f.credit.offer.request.slot, 0)
    );
    let slot_released_64 = w2[0].sequence;
    let j2 = s2.journal();
    assert_eq!(j2.records, 253, "FJ O2(64): {j2:?}");
    assert_all_visible(j2, "FJ O2(64): SlotReleased(64) published with lean 0");
    let states = lane.states();
    assert!(
        matches!(states.as_slice(), [(_, "in flight")]),
        "FJ: frame 65 admitted in T2(64), or a gap (no retry): {states:?}"
    );
    let job = states[0].0;
    assert_ne!(job, f.job);
    let (key, generation) = lane.admitted_key(job);
    assert_eq!(generation, FJ_BLOCK);
    let resource = resource_n(FJ_BLOCK);
    assert_eq!(
        j2.held,
        H + 2,
        "FJ O2(64): 64's holds spent, 65's taken: {j2:?}"
    );
    assert!(
        s2.releasing.is_empty(),
        "FJ O2(64): 64's slot release confirmed"
    );
    assert!(
        s2.facts.output_blocked,
        "FJ O2(64): the release was refused at 253"
    );
    assert_eq!(outbox_kinds(&s2.facts.outbox), ["ResourceReleased"]);
    assert_parked(&s2, 0, &f.release, "FJ O2(64)");
    let credit = credit_of(&s2, resource, "FJ O2(64)");
    assert!(credit.status_reserved);
    let identity = credit.offer.identity;
    let release = release_of(&credit, a.key.grant, resource);
    let baseline_published = s2.facts.owned_publication_sequence;
    assert_eq!(baseline_published, Some(slot_released_64));
    assert_eq!(s2.facts.lean_frames_in_flight, Some(1));
    s2.assert_stores_empty("FJ O2(64)");
    assert!(
        usage(&lane).backing > fill.backing_0,
        "FJ O2(64): 64 parked, 65 charged"
    );
    assert!(
        wake.wait(Some(Instant::now() + WAIT)).unwrap(),
        "FJ 65: the render result rings the owner"
    );
    assert_eq!(lane.leased(), 1);
    let c0 = d.c1.after_render();
    assert_eq!(counters(&lane), c0);
    eprintln!(
        "FJ O2(64): SlotReleased(64) {slot_released_64}, outcome(64) {}, journal {j2:?}",
        d.outcome
    );
    // T1(65): C3; the status and outcome queue behind the parked release.
    let late = lane.deadline_of(job) + MARGIN;
    budget(&fill, "FJ T1(65)");
    let (steps, observed) = lane.observed_turn(late, None);
    assert_eq!(steps, TURN);
    let c1 = c0.released();
    assert_eq!(
        observed,
        c1.unchanged(),
        "FJ T1(65): C3 declined at the content call"
    );
    let s1 = lane.snapshot(&a, "FJ O1");
    let j1 = s1.journal();
    assert!(
        window(&s2, &s1, "FJ T1(65)").is_empty(),
        "blocked: nothing commits"
    );
    assert!(s1.facts.output_blocked);
    assert_eq!((j1.records, j1.held), (253, H + 2), "{j1:?}");
    assert_eq!(
        outbox_kinds(&s1.facts.outbox),
        ["ResourceReleased", "ResourceStatus(2)", "FrameOutcome(3,9)"],
        "FJ O1: the parked release, then the frame's status and outcome"
    );
    assert_parked(&s1, 0, &f.release, "FJ O1");
    assert!(s1.facts.outbox[1].resource_settlement);
    assert_eq!(s1.releasing, [identity], "FJ O1: release owed");
    assert_eq!(
        s1.facts.owned_publication_sequence, baseline_published,
        "MD7: the release committed while its status was still queued"
    );
    assert!(credit_of(&s1, resource, "FJ O1").status_reserved);
    let early = store_release(&s1, &release, "FJ O1");
    assert!(
        usage(&lane).backing > fill.backing_0,
        "FJ O1: backing charged"
    );
    eprintln!(
        "FJ O1: leased {}, release pending in the store {early}, lean {:?} (the outcome is parked, not appended), journal {j1:?}",
        lane.leased(),
        s1.facts.lean_frames_in_flight
    );
    // PP1: nothing of frame 65 is readable; the peer's frontier is recorded.
    let (pp1, l1) = peer_poll(&mut a);
    assert_eq!(pp1.queued, fill.queued_0, "FJ: no ACK while withheld");
    assert!(
        l1.frames.is_empty() && l1.contents.is_empty(),
        "FJ PP1: {l1:?}"
    );
    assert!(peer_terminals(&a.frames, key, FJ_BLOCK).is_empty());
    assert!(statuses_for(&a.contents, a.key.grant, resource).is_empty());
    eprintln!("FJ PP1 {pp1:?}");
    // T2(65), the full turn after slot freedom: SlotReleased(65) refused
    // by the status guard; the store's release parks fourth.
    lane.slot_freed("FJ 65");
    budget(&fill, "FJ T2(65)");
    let (steps, observed) = lane.observed_turn(late, None);
    assert_eq!(steps, TURN);
    assert_eq!(observed, c1.unchanged(), "FJ T2(65): nothing more");
    let s2b = lane.snapshot(&a, "FJ O2");
    let j2b = s2b.journal();
    assert!(s2b.facts.output_blocked);
    assert!(window(&s1, &s2b, "FJ T2(65)").is_empty());
    assert_eq!(
        outbox_kinds(&s2b.facts.outbox),
        [
            "ResourceReleased",
            "ResourceStatus(2)",
            "FrameOutcome(3,9)",
            "ResourceReleased"
        ],
        "FJ O2: the store's release joined the parked FIFO"
    );
    assert_parked(&s2b, 0, &f.release, "FJ O2");
    assert_parked(&s2b, 3, &release, "FJ O2");
    assert_eq!(s2b.releasing, [identity]);
    assert_eq!(
        s2b.facts.owned_publication_sequence, baseline_published,
        "MD7: the release committed while its status was still queued"
    );
    assert!(credit_of(&s2b, resource, "FJ O2").status_reserved);
    s2b.assert_stores_empty("FJ O2");
    assert_eq!((j2b.records, j2b.held), (253, H + 2), "{j2b:?}");
    assert!(
        usage(&lane).backing > fill.backing_0,
        "FJ O2: backing still charged"
    );
    assert_eq!(lane.leased(), 0);
    assert_eq!(counters(&lane), c1);
    // The unblock: the ACK, sent by PP, applied by IT.
    budget(&fill, "FJ unblock");
    let acked = acknowledge(&mut a, &fill, &d, slot_released_64, "FJ");
    let (after_ack, after_drain, after_t3) = match acked {
        Acked::SlotReleased => (0, 4, 5),
        Acked::Outcome => (1, 5, 6),
    };
    budget(&fill, "FJ IT");
    lane.input_turn(&a);
    let s3 = lane.snapshot(&a, "FJ O3");
    let j3 = s3.journal();
    assert!(!s3.facts.output_blocked, "FJ O3: unblocked");
    assert!(
        j3.floor > j2b.floor,
        "FJ O3: the ACK released retention: {} -> {}",
        j2b.floor,
        j3.floor
    );
    let w3 = window(&s2b, &s3, "FJ IT");
    assert_eq!(
        kinds(&w3),
        [
            "ResourceReleased",
            "ResourceStatus(2)",
            "FrameOutcome(3,9)",
            "ResourceReleased"
        ],
        "FJ IT: the drain commits the parked four in FIFO order"
    );
    assert_eq!(w3[0].identity, release_identity(&f.release));
    let status = status_for(&w3, a.key.grant, resource, "FJ IT");
    assert_eq!(peer_terminals(&frames_in(&w3), key, FJ_BLOCK), [FAILED]);
    assert_eq!(w3[3].identity, release_identity(&release));
    assert_eq!(j3.records, after_drain, "FJ O3: {acked:?}: {j3:?}");
    assert_eq!(j3.records - 4, after_ack);
    assert_eq!(
        first_retained(&s3),
        Some(match acked {
            Acked::SlotReleased => w3[0].sequence,
            Acked::Outcome => slot_released_64,
        }),
        "FJ O3: the retention floor's record"
    );
    assert_eq!(
        j3.held,
        H + 1,
        "FJ O3: the status hold spent, the release hold live: {j3:?}"
    );
    assert_eq!(s3.releasing, [identity]);
    assert_eq!(
        credit_of(&s3, resource, "FJ O3"),
        OwnedCreditFacts {
            offer: credit.offer,
            status_reserved: false,
        }
    );
    assert_eq!(s3.facts.owned_publication_sequence, Some(status.sequence));
    assert!(s3.facts.outbox.is_empty());
    s3.assert_stores_empty("FJ O3");
    assert_all_visible(j3, "FJ O3");
    assert_eq!(s3.facts.lean_frames_in_flight, Some(0));
    assert_eq!(
        usage(&lane).backing,
        fill.backing_0,
        "FJ O3: both backings released"
    );
    eprintln!(
        "FJ O3: records {} bytes {} floor {}",
        j3.records, j3.bytes, j3.floor
    );
    // T3: SlotReleased after the release.
    budget(&fill, "FJ T3");
    let (steps, observed) = lane.observed_turn(late, None);
    assert_eq!(steps, TURN);
    assert_eq!(observed, c1.unchanged(), "FJ T3: nothing more");
    let s4 = lane.snapshot(&a, "FJ O4");
    let w4 = window(&s3, &s4, "FJ T3");
    assert_eq!(kinds(&w4), ["SlotReleased(0)"], "FJ T3: the slot's release");
    let event = slot_released_of(&w4[0]).unwrap();
    assert_eq!(
        (event.identity, event.resource, event.slot, event.reason),
        (identity, resource, credit.offer.request.slot, 0)
    );
    assert!(
        w3[3].sequence < w4[0].sequence,
        "ResourceReleased {} before SlotReleased {} on the ACK-unblocked path",
        w3[3].sequence,
        w4[0].sequence
    );
    let j4 = s4.journal();
    assert_eq!(j4.records, after_t3, "FJ O4: {acked:?}: {j4:?}");
    assert_eq!(j4.held, H, "FJ O4: back to the persistent hold: {j4:?}");
    assert!(s4.releasing.is_empty());
    assert!(
        !s4.facts
            .owned_live_credits
            .iter()
            .any(|c| c.offer == credit.offer)
    );
    assert_eq!(s4.facts.owned_publication_sequence, Some(w4[0].sequence));
    assert_all_visible(j4, "FJ O4");
    s4.assert_stores_empty("FJ O4");
    assert!(s4.facts.outbox.is_empty());
    assert_eq!(s4.facts.lean_frames_in_flight, Some(0));
    // The peer reads the rest.
    peer_reads_terminal(&mut a, FJ_BLOCK, "FJ");
    assert_eq!(a.terminals(FJ_BLOCK), [FAILED]);
    assert_eq!(
        statuses_for(&a.contents, a.key.grant, resource),
        [(2, status_of(status).unwrap().reason)]
    );
    for (which, rel) in [("64", &f.release), ("65", &release)] {
        assert_eq!(
            a.contents.iter().filter(|r| *r == rel).count(),
            1,
            "the release of {which} once"
        );
    }
    forget_observed(&mut fill, &mut a, "FJ end");
    assert!(lane.states().is_empty());
    assert_eq!(counters(&lane), c1);
    assert_eq!(lane.native.installs(), 0);
    assert_eq!(lane.native.ordinary_queues(), 0);
}

// FJ-stop, F4-O and BP, split for the file-size limit.
#[path = "combined_journal_clear.rs"]
mod clear;
#[path = "combined_journal_offers.rs"]
mod offers;
#[path = "combined_journal_stop.rs"]
mod stop;
