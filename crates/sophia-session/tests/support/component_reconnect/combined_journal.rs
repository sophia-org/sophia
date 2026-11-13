//! G3-5d d-2b3d-2 Session controls over the parent's lane, observers,
//! windows and peer polls: the full journal under a withheld ACK (FJ, with
//! MD7's full-journal consumer), the next admission's Stop at the
//! reservation refusal (FJ-stop), the declared completion of F4's queued
//! fallback (F4-O, a simulated completion) and the nonempty outbox cleared
//! by disconnect (BP). Design REV3 D3/D5 and REV4 D3/D4-O as corrected by
//! the accounting REV6 and its clean parity table.
//!
//! Journal capacity (files/journal.rs `limits`, `append_limits`,
//! `hold_reservation`): credited records fit while `records < 256 − held`,
//! uncredited while `records < 192`; a reservation is taken under the
//! credited rule. `held` is a total. One reservation is persistent in this
//! lane, the channel-close record held since ChannelReady (export/image.rs),
//! so the baseline is H = 1 and an admitted offer makes it 3: the status
//! hold (spent by the ResourceStatus in T1) and the release hold (spent by
//! SlotReleased). The byte bound of the bar profile (files.rs: 65536 bytes,
//! 12800 reserved, every record here at most 200 bytes) binds after the
//! record bound for this inventory; `bytes` and `held` are printed at every
//! boundary rather than derived.
//!
//! A lean C3 filler journals FOUR records (validation-03 F3): the status
//! (reserved) and FrameOutcome(3, 9 RendererFailed) (credited) in T1; after
//! slot freedom SlotReleased (reserved, T2 images) and ResourceReleased
//! (credited, T2 content: `collect` after the lease release, flushed in the
//! Resources stage, journaled by the same pass; `confirm_isb_release` only
//! then releases the backing charge). ΔR = +4, measured after the IT that
//! follows T2. Holds 1 → 3 → 2 → 1; the ring's publication counter changes
//! to the status in T1 and to SlotReleased in T2. Each filler completes
//! only if it starts at R ≤ 251.
//!
//! The fill, driven by observed R: phase 0 takes the ACK baseline with
//! automatic ACKs still on and then disables them (the peer still reads;
//! its queued ACK must not move until the unblock); phase B grants three
//! spare allocations (+2 each: an uncredited Submitted and a credited
//! AllocationResult); phase A runs request/reject cycles (+2 each) while
//! R ≤ 191, to P; phase C runs k_C fillers. The clean table (REV6 review):
//! FJ odd R_B → P 191, target 251, 15 fillers; even → 188, 252, 16;
//! FJ-stop odd → 191, 251, 15; even → 190, 250, 15. FJ's target is the
//! blocking frame's start; FJ-stop's is the final filler's start, which
//! ends at 254 or 255. k_A = (P − R_B) / 2 must be a whole count.
//!
//! FJ: the blocking frame k admitted at R_T0 ∈ {251, 252} holds 3; J =
//! 254 − R_T0 spare invalidations are queued between T0 and T1 without a
//! drain; T1's first transport pass commits J − 1 of them and is refused at
//! the last (records 253, held 3, `output_blocked`), the frame's status and
//! outcome queue behind it. The slot is freed before T2 so the store's
//! ResourceReleased reaches the outbox fourth in T2. At O1 the store's
//! event queue is empty or holds exactly this frame's release, because the
//! worker may consume the Release before the decline's own collect; it
//! cannot be in the outbox at O1, since the Resources stage flushes before
//! the arm runs. An ACK unblocks: the drain commits the four in FIFO order,
//! T3 commits SlotReleased, so on this path ResourceReleased precedes
//! SlotReleased, the reverse of a completed filler. MD7 (delete the
//! `credit.status.is_some()` guard in owned_ring/release.rs) is caught by
//! the parent R4's order assertion first; here its full-journal consumer
//! is the blocked frame's publication counter, which must still equal the
//! saved pre-block baseline at O1 and O2 while the status is parked.
//!
//! Timing: `check_ack_progress` closes a blocked reader 2 s after the
//! journal's `last_progress`, which only an ACK advances. Before EVERY
//! input-serving pass of a fill, blocked or not, the budget 2 s − 200 ms
//! from the baseline is asserted; a miss is the named failure, never a
//! retry. The turns inside `admitted` are covered by the check before and
//! after it.
//!
//! Predictions until this harness runs: R after every phase, the
//! reachability of the targets within the budget, every window, the
//! parked inventories, the hold counts, the Stop and its unblock, the
//! simulated completion's response and the clear. Backing destruction is
//! not observed; a journal commit is neither peer consumption nor a
//! renderer acknowledgement; the renderer lease is a third domain.
//! Off/native-session remain unpassed and unwaived.
use super::*;
use sophia_protocol::shell_files::{
    SHELL_FILE_ACK_PROGRESS_TIMEOUT_MILLIS, SHELL_FILE_MAX_JOURNAL_RECORDS,
    SHELL_FILE_TERMINAL_RESERVE_RECORDS,
};
use sophia_runtime::{ContentAllocationError, ContentMemoryUsage, OutboxEntryFacts};
use sophia_shell_client::Custody;

/// The protocol's journal record limit and its uncredited limit, asserted
/// against the constants in phase 0.
const MAX_RECORDS: usize = 256;
const UNCREDITED_LIMIT: usize = 192;
/// The persistent channel-close reservation: total holds between fillers.
const H: usize = 1;
/// Spare allocations granted in phase B.
const SPARES: u64 = 3;
/// The peer's allocation requests go under one transaction, as `send`.
const REQUEST: TransactionId = TransactionId::from_raw(10);
/// Spare invalidations: 60 + j.
const INVALIDATE_BASE: u64 = 60;
const PRESENTED: (u16, u16) = (2, ContentReason::None as u16);

/// The fill's fixed facts from phase 0.
struct Fill {
    /// The journal's `last_progress` at the baseline: the budget's origin.
    progress_0: Instant,
    /// The peer's queued ACK at the baseline, unchanged until the unblock.
    queued_0: u64,
    /// The backing charge with nothing admitted.
    backing_0: u64,
    /// The spare allocations A_1.., in grant order.
    spares: Vec<ContentAllocationId>,
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

/// An outbox entry's AllocationResult as (transaction, allocation).
fn outbox_result(entry: &OutboxEntryFacts, label: &str) -> (TransactionId, ContentAllocationId) {
    let OutboxRecordFacts::Content(transaction, ShellContentRecord::AllocationResult(result)) =
        &entry.record
    else {
        panic!("{label}: an AllocationResult: {entry:?}")
    };
    (*transaction, result.allocation)
}

/// Phase 0, after `attempt`: the ACK baseline with automatic ACKs on (one
/// peer poll and one IT apply any ACK already queued), then automatic ACKs
/// off. Asserts the protocol limits and the persistent hold.
fn baseline(lane: &mut Lane, a: &mut Attempt, label: &str) -> Fill {
    assert_eq!(MAX_RECORDS, usize::from(SHELL_FILE_MAX_JOURNAL_RECORDS));
    assert_eq!(
        UNCREDITED_LIMIT,
        MAX_RECORDS - usize::from(SHELL_FILE_TERMINAL_RESERVE_RECORDS)
    );
    let p0 = a.peer.journal_progress();
    a.peer.poll_io().unwrap();
    observe_peer(a);
    lane.input_turn(a);
    let (pp, _) = peer_poll(a);
    let s0 = lane.snapshot(a, label);
    let j = s0.journal();
    assert_eq!(
        j.held, H,
        "{label}: the persistent channel-close hold alone: {j:?}"
    );
    assert!(
        !pp.ack_pending && pp.queued == pp.acknowledged,
        "{label}: no ACK in flight: {pp:?}"
    );
    assert!(!s0.facts.output_blocked);
    a.peer.set_automatic_journal_ack(false).unwrap();
    let usage_0 = usage(lane);
    eprintln!("{label}: ACK baseline {p0:?} -> {pp:?}; journal {j:?}; usage {usage_0:?}");
    Fill {
        progress_0: j.last_progress,
        queued_0: pp.queued,
        backing_0: usage_0.backing,
        spares: Vec::new(),
    }
}

/// R and held, read between calls.
fn records(lane: &Lane, a: &Attempt, label: &str) -> (usize, usize) {
    let s = lane.snapshot(a, label);
    let j = s.journal();
    (j.records, j.held)
}

/// The peer's AllocationRequest `id` (one uncredited Submitted, so R must
/// be below 192), flushed by `send`'s passes with the budget checked
/// before each; then the owner takes it from the store.
fn request(lane: &mut Lane, a: &mut Attempt, fill: &Fill, id: u64, label: &str) {
    let (r, _) = records(lane, a, label);
    assert!(
        r < UNCREDITED_LIMIT,
        "{label}: R {r} leaves no room for an uncredited Submitted"
    );
    let record = ShellContentRecord::AllocationRequest(ContentAllocationRequest {
        grant: a.key.grant,
        output: OUTPUT,
        allocation_request_id: id,
        operation: 1,
        role: 1,
        edge: 1,
        prior: ContentAllocationId::default(),
        parent: ContentAllocationId::default(),
        parent_presentation_epoch: 0,
        anchor_parent_rect: ContentPixelRect::default(),
        desired_width: 64,
        desired_height: 1,
        margins: ContentMargins::default(),
    });
    let admission = a.peer.enqueue_content_tracked(REQUEST, &record).unwrap();
    let deadline = Instant::now() + WAIT;
    loop {
        budget(fill, label);
        a.peer.poll_io().unwrap();
        lane.h
            .owner
            .with_service(a.key, |_, t| {
                t.service_owner_turn().unwrap();
                t.service_content_resources(content_now_msec()).unwrap();
                t.poll_io().unwrap();
            })
            .unwrap();
        let mut complete = true;
        for ticket in admission.tickets() {
            match a.peer.custody(ticket).unwrap() {
                Custody::Submitted | Custody::Stored => {}
                Custody::Queued | Custody::InFlight => complete = false,
                other => panic!("{label}: request custody {other:?}"),
            }
        }
        if complete {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "{label}: request {id} was never submitted"
        );
        std::thread::yield_now();
    }
    observe_peer(a);
    lane.h
        .owner
        .with_service(a.key, |_, t| {
            t.service_content_allocation_requests(&[], content_now_msec())
                .unwrap();
            let (_, pending) = t
                .next_content_allocation_request()
                .expect("the request is pending");
            assert_eq!(
                pending.allocation_request_id, id,
                "{label}: the owner holds request {id}"
            );
        })
        .unwrap();
}

/// One request/reject cycle: the Submitted, then the rejection's credited
/// AllocationResult drained by one IT. Predicted ΔR +2; R after.
fn cycle(lane: &mut Lane, a: &mut Attempt, fill: &Fill, id: u64, label: &str) -> usize {
    let (before, _) = records(lane, a, label);
    request(lane, a, fill, id, label);
    lane.h
        .owner
        .with_service(a.key, |_, t| {
            t.reject_content_allocation(id, ContentAllocationError::Budget)
        })
        .unwrap()
        .unwrap();
    budget(fill, label);
    lane.input_turn(a);
    let (after, held) = records(lane, a, label);
    assert_eq!(
        after,
        before + 2,
        "{label}: a cycle journals Submitted and AllocationResult"
    );
    assert_eq!(held, H, "{label}: no hold");
    after
}

/// A spare allocation A_j granted to request `id`: the Submitted, then the
/// grant's credited AllocationResult drained by one IT. Predicted ΔR +2.
fn grant_spare(lane: &mut Lane, a: &mut Attempt, fill: &mut Fill, id: u64, j: u64, label: &str) {
    let allocation = ContentAllocationId {
        id: ALLOCATION.id + j,
        generation: 1,
    };
    let (before, _) = records(lane, a, label);
    request(lane, a, fill, id, label);
    lane.h
        .owner
        .with_service(a.key, |_, t| {
            t.grant_content_allocation(
                id,
                ContentAllocationSnapshot {
                    native_opening: None,
                    output: OUTPUT,
                    allocation,
                    scale_generation: 1,
                    scale_numerator: 1,
                    scale_denominator: 1,
                    role: 1,
                    edge: 1,
                    margins: ContentMargins::default(),
                    logical: ContentLogicalRect {
                        x: 0,
                        y: 0,
                        width: 64,
                        height: 1,
                    },
                    pixel: ContentPixelRect {
                        x: 0,
                        y: 0,
                        width: 64,
                        height: 1,
                    },
                    parent: ContentAllocationId::default(),
                    anchor_parent_rect: ContentPixelRect::default(),
                    allowed_reservation_extent: 1,
                },
                &[],
            )
        })
        .unwrap()
        .unwrap();
    budget(fill, label);
    lane.input_turn(a);
    let (after, _) = records(lane, a, label);
    assert_eq!(
        after,
        before + 2,
        "{label}: a grant journals Submitted and AllocationResult"
    );
    fill.spares.push(allocation);
}

/// Phase B: the three spares, ids 2..4, beside the frame's own allocation.
/// R after.
fn phase_b(lane: &mut Lane, a: &mut Attempt, fill: &mut Fill, label: &str) -> usize {
    let (r0, _) = records(lane, a, label);
    for j in 1..=SPARES {
        grant_spare(lane, a, fill, 1 + j, j, &format!("{label} spare {j}"));
    }
    let (r, _) = records(lane, a, label);
    assert_eq!(r, r0 + 6, "{label} phase B: ΔR +6");
    let mut granted: Vec<(u64, u64)> = lane
        .h
        .owner
        .with_service(a.key, |_, t| t.content_allocation_snapshots())
        .unwrap()
        .iter()
        .map(|snapshot| (snapshot.allocation.id, snapshot.allocation.generation))
        .collect();
    granted.sort_unstable();
    let mut expected: Vec<(u64, u64)> = std::iter::once(ALLOCATION)
        .chain(fill.spares.iter().copied())
        .map(|allocation| (allocation.id, allocation.generation))
        .collect();
    expected.sort_unstable();
    assert_eq!(
        granted, expected,
        "{label}: the frame's allocation and the spares"
    );
    eprintln!("{label} phase B: R_B {r}, spares {:?}", fill.spares);
    r
}

/// The clean parity table (REV6 review): P, the target and the filler
/// count from the observed R_B. FJ's target is the blocking frame's start;
/// FJ-stop's is the final filler's start.
struct Plan {
    p: usize,
    target: usize,
    k_a: usize,
    k_c: usize,
}

fn plan(r_b: usize, stop: bool) -> Plan {
    let (p, target) = match (r_b % 2 == 1, stop) {
        (true, _) => (191, 251),
        (false, false) => (188, 252),
        (false, true) => (190, 250),
    };
    assert!(
        p >= r_b && (p - r_b) % 2 == 0,
        "R_B {r_b} reaches P {p} by whole cycles"
    );
    assert_eq!(
        (target - p) % 4,
        0,
        "P {p} reaches {target} by whole fillers"
    );
    let k_c = (target - p) / 4;
    assert!(k_c <= 16, "at most 16 completed fillers");
    Plan {
        p,
        target,
        k_a: (p - r_b) / 2,
        k_c,
    }
}

/// Phase A: k_A cycles with ids 5.., to P. R after.
fn phase_a(lane: &mut Lane, a: &mut Attempt, fill: &Fill, plan: &Plan, label: &str) -> usize {
    let (mut r, _) = records(lane, a, label);
    for i in 0..plan.k_a {
        let id = 5 + u64::try_from(i).unwrap();
        r = cycle(lane, a, fill, id, &format!("{label} cycle {i}"));
    }
    assert_eq!(r, plan.p, "{label} phase A: R reached P");
    r
}

/// Frame `n` through the C3 fill path (F3's accepted path) to source
/// release, backing retirement, row disposal and a free renderer slot,
/// with the four records identified and every credit transition asserted.
/// R after its IT.
fn filler(
    lane: &mut Lane,
    wake: &sophia_wake::Wake,
    a: &mut Attempt,
    fill: &Fill,
    n: u64,
    label: &str,
) -> usize {
    let count = usize::try_from(n).unwrap();
    budget(fill, label);
    lane.submit(a, n, PIXEL);
    let job = lane.admitted(a);
    assert!(
        wake.wait(Some(Instant::now() + WAIT)).unwrap(),
        "{label}: the render result rings the owner"
    );
    assert_eq!(lane.leased(), 1, "{label}: the result holds its slot");
    let (key, generation) = lane.admitted_key(job);
    assert_eq!(generation, n);
    let resource = resource_n(n);
    let s0 = lane.snapshot(a, &format!("{label} O0"));
    let j0 = s0.journal();
    let r0 = j0.records;
    assert_eq!(j0.held, H + 2, "{label} O0: the offer's two holds: {j0:?}");
    let credit = credit_of(&s0, resource, &format!("{label} O0"));
    assert!(credit.status_reserved);
    let identity = credit.offer.identity;
    let published_0 = s0.facts.owned_publication_sequence;
    let release = release_of(&credit, a.key.grant, resource);
    assert!(s0.releasing.is_empty());
    let usage_0 = usage(lane);
    assert!(
        usage_0.backing > fill.backing_0,
        "{label} O0: the admission charged the backing: {usage_0:?}"
    );
    let late = lane.deadline_of(job) + MARGIN;
    budget(fill, &format!("{label} T1"));
    let (steps, observed) = lane.observed_turn(late, None);
    assert_eq!(steps, TURN);
    assert_eq!(
        observed, DECLINED,
        "{label}: C3 declined at the content call"
    );
    let s1 = lane.snapshot(a, &format!("{label} O1"));
    let w1 = window(&s0, &s1, &format!("{label} T1"));
    assert_eq!(
        kinds(&w1),
        ["ResourceStatus(2)", "FrameOutcome(3,9)"],
        "{label} T1: the acceptance, then the decline's terminal"
    );
    let status = status_for(&w1, a.key.grant, resource, &format!("{label} T1"));
    assert_eq!(peer_terminals(&frames_in(&w1), key, n), [FAILED]);
    let j1 = s1.journal();
    assert_eq!(
        j1.held,
        H + 1,
        "{label} O1: the status hold is spent: {j1:?}"
    );
    assert_eq!(s1.releasing, [identity], "{label} O1: release owed");
    assert_eq!(
        credit_of(&s1, resource, &format!("{label} O1")),
        OwnedCreditFacts {
            offer: credit.offer,
            status_reserved: false,
        }
    );
    assert_eq!(
        s1.facts.owned_publication_sequence,
        Some(status.sequence),
        "{label} O1: the status is the ring's last publication (REV6 B3)"
    );
    assert_ne!(s1.facts.owned_publication_sequence, published_0);
    assert!(s1.facts.outbox.is_empty(), "{:?}", s1.facts.outbox);
    assert!(!s1.facts.output_blocked);
    assert_all_visible(j1, &format!("{label} O1"));
    let early = store_release(&s1, &release, &format!("{label} O1"));
    assert!(
        usage(lane).backing > fill.backing_0,
        "{label} O1: the backing stays charged until the release is journaled"
    );
    eprintln!(
        "{label} O1: leased {}, release pending in the store {early}, journal {j1:?}",
        lane.leased()
    );
    peer_reads_terminal(a, n, label);
    assert_eq!(
        a.peer.journal_progress().queued,
        fill.queued_0,
        "{label}: no ACK while withheld"
    );
    lane.slot_freed(label);
    budget(fill, &format!("{label} T2"));
    let (steps, observed) = lane.observed_turn(late, None);
    assert_eq!(steps, TURN);
    assert_eq!(observed, DECLINED, "{label}: nothing more");
    let s2 = lane.snapshot(a, &format!("{label} O2"));
    let w2 = window(&s1, &s2, &format!("{label} T2"));
    assert_eq!(
        kinds(&w2),
        ["SlotReleased(0)", "ResourceReleased"],
        "{label} T2: the slot's release, then the backing's retirement"
    );
    let event = slot_released_of(&w2[0]).unwrap();
    assert_eq!(
        (event.identity, event.resource, event.slot, event.reason),
        (identity, resource, credit.offer.request.slot, 0),
        "{label}: the original source's release"
    );
    assert_eq!(
        w2[1].identity,
        RecordIdentity::Content {
            transaction: release.0,
            record: release.1.clone()
        },
        "{label}: the release joined to the resource's admission"
    );
    let j2 = s2.journal();
    assert_eq!(j2.held, H, "{label} O2: both offer holds spent: {j2:?}");
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
        std::slice::from_ref(&release),
        "{label}: the peer reads the release on the content queue"
    );
    assert!(l.frames.is_empty(), "{label}: {l:?}");
    assert_eq!(a.terminals(n), [FAILED]);
    assert!(lane.states().is_empty(), "{label}: the row is clear");
    assert_eq!(lane.releases(), (count, count));
    assert_eq!(lane.native.installs(), 0);
    assert_eq!(lane.native.ordinary_queues(), 0);
    assert_eq!(lane.acquisitions(), count, "{label}: no later render");
    let r = s3.journal().records;
    assert_eq!(
        r,
        r0 + 4,
        "{label}: ΔR +4: {:?}",
        kinds(&window(&s0, &s3, &format!("{label} total")))
    );
    eprintln!(
        "{label}: R {r0} -> {r}, bytes {} held {}",
        s3.journal().bytes,
        s3.journal().held
    );
    r
}

/// Phase C: fillers 1..=k_C. R after the last.
fn phase_c(
    lane: &mut Lane,
    wake: &sophia_wake::Wake,
    a: &mut Attempt,
    fill: &Fill,
    k_c: usize,
    label: &str,
) -> usize {
    let (mut r, _) = records(lane, a, label);
    for n in 1..=u64::try_from(k_c).unwrap() {
        r = filler(lane, wake, a, fill, n, &format!("{label} filler {n}"));
    }
    r
}

/// FJ: the fill to the blocking frame, the refused AllocationResult at the
/// FIFO head with the frame's status, outcome and release parked behind
/// it, the ring's publication counter unchanged while the status is
/// parked (MD7's full-journal consumer), the ACK that unblocks, the
/// four-record drain, then SlotReleased.
#[test]
fn fj_a_refused_allocation_result_parks_the_frames_records_until_an_ack() {
    let (mut lane, wake) = Lane::running();
    let mut a = lane.attempt();
    let mut fill = baseline(&mut lane, &mut a, "FJ phase 0");
    let r_b = phase_b(&mut lane, &mut a, &mut fill, "FJ");
    let plan = plan(r_b, false);
    eprintln!(
        "FJ plan: R_B {r_b} P {} target {} k_A {} k_C {}",
        plan.p, plan.target, plan.k_a, plan.k_c
    );
    phase_a(&mut lane, &mut a, &fill, &plan, "FJ");
    let r_c = phase_c(&mut lane, &wake, &mut a, &fill, plan.k_c, "FJ");
    assert_eq!(r_c, plan.target, "FJ phase C: R reached the target");
    // The blocking frame k.
    let k = u64::try_from(plan.k_c).unwrap() + 1;
    let count = usize::try_from(k).unwrap();
    budget(&fill, "FJ k");
    lane.submit(&mut a, k, PIXEL);
    let job = lane.admitted(&mut a);
    assert!(wake.wait(Some(Instant::now() + WAIT)).unwrap());
    assert_eq!(lane.leased(), 1);
    let (key, _) = lane.admitted_key(job);
    let resource = resource_n(k);
    let st0 = lane.snapshot(&a, "FJ T0");
    let jt0 = st0.journal();
    assert_eq!(jt0.records, plan.target, "FJ T0: R_T0");
    assert_eq!(jt0.held, H + 2, "{jt0:?}");
    let credit = credit_of(&st0, resource, "FJ T0");
    assert!(credit.status_reserved);
    let identity = credit.offer.identity;
    let baseline_published = st0.facts.owned_publication_sequence;
    let release = release_of(&credit, a.key.grant, resource);
    assert!(usage(&lane).backing > fill.backing_0);
    // J spare invalidations, queued without a drain.
    let j_count = 254 - plan.target;
    assert!((2..=fill.spares.len()).contains(&j_count), "J {j_count}");
    let mut invalidations = Vec::new();
    for j in 0..j_count {
        let transaction = TransactionId::from_raw(INVALIDATE_BASE + u64::try_from(j).unwrap());
        let spare = fill.spares[j];
        lane.h
            .owner
            .with_service(a.key, |_, t| {
                t.invalidate_content_allocation(transaction, spare, ContentReason::Revoked)
            })
            .unwrap()
            .unwrap();
        invalidations.push((transaction, spare));
        let s = lane.snapshot(&a, "FJ invalidation");
        assert_eq!(
            committed(s.journal()),
            committed(jt0),
            "invalidation {j}: journals nothing itself"
        );
        assert_eq!(s.facts.outbox.len(), j + 1, "{:?}", s.facts.outbox);
        assert_eq!(
            outbox_result(&s.facts.outbox[j], "FJ invalidation"),
            (transaction, spare)
        );
        assert!(!s.facts.output_blocked);
        assert!(
            s.stores.allocations.is_empty(),
            "transferred without a drain"
        );
    }
    let s_pre = lane.snapshot(&a, "FJ pre-T1");
    let late = lane.deadline_of(job) + MARGIN;
    budget(&fill, "FJ T1");
    let (steps, observed) = lane.observed_turn(late, None);
    assert_eq!(steps, TURN);
    assert_eq!(observed, DECLINED, "FJ: C3 declined at the content call");
    // O1: J − 1 invalidations committed, the last refused at the head.
    let s1 = lane.snapshot(&a, "FJ O1");
    let j1 = s1.journal();
    assert!(s1.facts.output_blocked, "FJ O1: the drain was refused");
    let w1 = window(&s_pre, &s1, "FJ T1");
    assert_eq!(
        kinds(&w1),
        vec!["AllocationResult".to_string(); j_count - 1],
        "FJ T1: J - 1 invalidations committed"
    );
    for (record, (transaction, spare)) in w1.iter().zip(&invalidations) {
        let (t, result) = allocation_result_of(record).unwrap();
        assert_eq!((t, result.allocation), (*transaction, *spare));
    }
    assert_eq!(j1.records, 253, "FJ O1: blocked at 256 - held: {j1:?}");
    assert_eq!(j1.held, H + 2, "{j1:?}");
    assert_eq!(
        outbox_kinds(&s1.facts.outbox),
        ["AllocationResult", "ResourceStatus(2)", "FrameOutcome(3,9)"],
        "FJ O1: the refused head, then the frame's status and outcome"
    );
    assert_eq!(
        outbox_result(&s1.facts.outbox[0], "FJ O1"),
        invalidations[j_count - 1],
        "FJ O1: the last invalidation is the refused head"
    );
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
        "FJ O1: leased {}, release pending in the store {early}, deferred {:?}, lean {:?}, journal {j1:?}",
        lane.leased(),
        j1.deferred,
        s1.facts.lean_frames_in_flight
    );
    // PP1: the committed invalidations are readable; the frame's are not.
    let (pp1, l1) = peer_poll(&mut a);
    assert_eq!(pp1.queued, fill.queued_0, "FJ: no ACK while withheld");
    let read: Vec<(TransactionId, ContentAllocationId)> = l1
        .contents
        .iter()
        .filter_map(|(t, record)| match record {
            ShellContentRecord::AllocationResult(result) => Some((*t, result.allocation)),
            _ => None,
        })
        .collect();
    assert_eq!(read, invalidations[..j_count - 1], "FJ PP1: {l1:?}");
    assert_eq!(l1.len(), j_count - 1, "FJ PP1: nothing else: {l1:?}");
    assert!(peer_terminals(&a.frames, key, k).is_empty());
    assert!(statuses_for(&a.contents, a.key.grant, resource).is_empty());
    lane.slot_freed("FJ k");
    budget(&fill, "FJ T2");
    let (steps, observed) = lane.observed_turn(late, None);
    assert_eq!(steps, TURN);
    assert_eq!(observed, DECLINED, "FJ T2: nothing more");
    // O2: the release behind the parked outcome; still blocked.
    let s2 = lane.snapshot(&a, "FJ O2");
    let j2 = s2.journal();
    assert!(s2.facts.output_blocked);
    assert!(window(&s1, &s2, "FJ T2").is_empty());
    assert_eq!(
        outbox_kinds(&s2.facts.outbox),
        [
            "AllocationResult",
            "ResourceStatus(2)",
            "FrameOutcome(3,9)",
            "ResourceReleased"
        ],
        "FJ O2: the store's release joined the parked FIFO"
    );
    assert_eq!(
        s2.facts.outbox[3].record,
        OutboxRecordFacts::Content(release.0, release.1.clone()),
        "FJ O2: the exact release"
    );
    assert_eq!(s2.releasing, [identity]);
    assert_eq!(
        s2.facts.owned_publication_sequence, baseline_published,
        "MD7: the release committed while its status was still queued"
    );
    assert!(credit_of(&s2, resource, "FJ O2").status_reserved);
    s2.assert_stores_empty("FJ O2");
    assert_eq!((j2.records, j2.held), (253, H + 2), "{j2:?}");
    assert!(
        usage(&lane).backing > fill.backing_0,
        "FJ O2: backing still charged"
    );
    assert_eq!(lane.leased(), 0);
    assert_eq!(lane.releases(), (count, count));
    // The unblock: the ACK, sent by PP, applied by IT.
    budget(&fill, "FJ unblock");
    assert!(
        a.peer.acknowledge_journal().unwrap(),
        "an ACK queued through the safe limit"
    );
    let (pp2, l2) = peer_poll(&mut a);
    assert!(pp2.queued > fill.queued_0, "the ACK was sent: {pp2:?}");
    assert!(l2.is_empty(), "nothing new to read: {l2:?}");
    budget(&fill, "FJ IT");
    lane.input_turn(&mut a);
    let s3 = lane.snapshot(&a, "FJ O3");
    let j3 = s3.journal();
    assert!(!s3.facts.output_blocked, "FJ O3: unblocked");
    assert!(
        j3.floor > j2.floor,
        "FJ O3: the ACK released retention: {} -> {}",
        j2.floor,
        j3.floor
    );
    let w3 = window(&s2, &s3, "FJ IT");
    assert_eq!(
        kinds(&w3),
        [
            "AllocationResult",
            "ResourceStatus(2)",
            "FrameOutcome(3,9)",
            "ResourceReleased"
        ],
        "FJ IT: the drain commits the parked four in FIFO order"
    );
    let (t, result) = allocation_result_of(&w3[0]).unwrap();
    assert_eq!((t, result.allocation), invalidations[j_count - 1]);
    let status = status_for(&w3, a.key.grant, resource, "FJ IT");
    assert_eq!(peer_terminals(&frames_in(&w3), key, k), [FAILED]);
    assert_eq!(
        w3[3].identity,
        RecordIdentity::Content {
            transaction: release.0,
            record: release.1.clone()
        }
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
    assert_eq!(
        usage(&lane).backing,
        fill.backing_0,
        "FJ O3: backing released"
    );
    eprintln!(
        "FJ O3: records {} bytes {} floor {}",
        j3.records, j3.bytes, j3.floor
    );
    // T3: SlotReleased after the release.
    budget(&fill, "FJ T3");
    let (steps, observed) = lane.observed_turn(late, None);
    assert_eq!(steps, TURN);
    assert_eq!(observed, DECLINED, "FJ T3: nothing more");
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
    peer_reads_terminal(&mut a, k, "FJ");
    assert_eq!(a.terminals(k), [FAILED]);
    assert_eq!(
        statuses_for(&a.contents, a.key.grant, resource),
        [(2, status_of(status).unwrap().reason)]
    );
    assert_eq!(
        a.contents.iter().filter(|r| **r == release).count(),
        1,
        "the release once"
    );
    assert!(lane.states().is_empty());
    assert_eq!(lane.releases(), (count, count));
    assert_eq!(lane.native.installs(), 0);
    assert_eq!(lane.native.ordinary_queues(), 0);
}

/// FJ-stop: the fill to a completed final filler at R ∈ {254, 255}, then
/// the next admission refused at its reservation hold: `Step::Stop`
/// (owned_copy/frame.rs), the offer waits, nothing is appended, holds
/// return to the persistent one. Which hold refuses follows the source's
/// order (offer.rs: status first, then release): at 254 the release hold,
/// at 255 the status hold; it is derived from R, not observed. The other
/// blockers are shown absent first. An ACK, then a turn, admits it.
#[test]
fn fj_stop_the_next_admission_stops_at_the_reservation_refusal_until_an_ack() {
    let (mut lane, wake) = Lane::running();
    let mut a = lane.attempt();
    let mut fill = baseline(&mut lane, &mut a, "FJ-stop phase 0");
    let r_b = phase_b(&mut lane, &mut a, &mut fill, "FJ-stop");
    let plan = plan(r_b, true);
    eprintln!(
        "FJ-stop plan: R_B {r_b} P {} final filler start {} k_A {} k_C {}",
        plan.p, plan.target, plan.k_a, plan.k_c
    );
    phase_a(&mut lane, &mut a, &fill, &plan, "FJ-stop");
    let r_final = phase_c(&mut lane, &wake, &mut a, &fill, plan.k_c, "FJ-stop");
    assert_eq!(
        r_final,
        plan.target + 4,
        "FJ-stop: the final filler completed"
    );
    assert!([254, 255].contains(&r_final), "R_final {r_final}");
    // No other blocker.
    let s = lane.snapshot(&a, "FJ-stop before");
    let j = s.journal();
    assert_eq!(j.held, H, "{j:?}");
    assert!(s.facts.owned_live_credits.is_empty());
    assert_eq!(s.facts.owned_pending_offer, None);
    assert!(!s.facts.output_blocked);
    assert_eq!(lane.h.owner.processes.image_pending(0), 0, "no image work");
    assert_eq!(lane.leased(), 0);
    assert_eq!(usage(&lane).backing, fill.backing_0);
    assert!(lane.states().is_empty());
    assert!(s.releasing.is_empty());
    let k = u64::try_from(plan.k_c).unwrap() + 1;
    let resource = resource_n(k);
    lane.submit(&mut a, k, PIXEL);
    // Turns until the owner holds the offer unadmitted: each is a Stop.
    let deadline = Instant::now() + WAIT;
    let mut turns = 0;
    let s_stop = loop {
        budget(&fill, "FJ-stop admission");
        a.peer.poll_io().unwrap();
        let (steps, accepted) = lane.quiet_driver_turn_at(Instant::now());
        assert_eq!(steps, TURN);
        assert_eq!(accepted, [0]);
        observe_peer(&mut a);
        turns += 1;
        let s = lane.snapshot(&a, "FJ-stop stop");
        if s.facts.owned_pending_offer.is_some() {
            break s;
        }
        assert!(
            Instant::now() < deadline,
            "FJ-stop: the offer never reached the owner: {:?}",
            lane.states()
        );
        std::thread::yield_now();
    };
    let j_stop = s_stop.journal();
    let pending = s_stop.facts.owned_pending_offer.unwrap();
    assert_eq!(pending.request.resource, resource, "the offer waits");
    assert!(
        lane.states().is_empty(),
        "no admission: {:?}",
        lane.states()
    );
    assert!(
        s_stop.facts.owned_live_credits.is_empty(),
        "the status hold, if taken, was released: {:?}",
        s_stop.facts.owned_live_credits
    );
    assert_eq!(j_stop.held, H, "{j_stop:?}");
    assert_eq!(committed(j_stop), committed(j), "nothing appended");
    assert_eq!(j_stop.records, r_final);
    assert!(
        !s_stop.facts.output_blocked,
        "nothing was refused in a drain"
    );
    assert_eq!(lane.h.owner.processes.image_pending(0), 0);
    assert_eq!(lane.leased(), 0);
    let refused = if r_final == 254 {
        "the release hold, after the status hold was taken and released"
    } else {
        "the status hold"
    };
    eprintln!(
        "FJ-stop: R {r_final}, {turns} turn(s); by the source's hold order {refused} refused; journal {j_stop:?}"
    );
    // The unblock: ACK, PP, IT, one turn: admitted.
    budget(&fill, "FJ-stop unblock");
    assert!(a.peer.acknowledge_journal().unwrap());
    let (pp, _) = peer_poll(&mut a);
    assert!(pp.queued > fill.queued_0, "{pp:?}");
    budget(&fill, "FJ-stop IT");
    lane.input_turn(&mut a);
    let s_u = lane.snapshot(&a, "FJ-stop after IT");
    assert!(
        s_u.journal().floor > j_stop.floor,
        "the ACK released retention"
    );
    assert!(s_u.journal().records < r_final);
    let job = lane.admitted(&mut a);
    let s_adm = lane.snapshot(&a, "FJ-stop admitted");
    assert_eq!(s_adm.journal().held, H + 2, "{:?}", s_adm.journal());
    assert_eq!(s_adm.facts.owned_pending_offer, None);
    assert!(credit_of(&s_adm, resource, "FJ-stop admitted").status_reserved);
    assert!(wake.wait(Some(Instant::now() + WAIT)).unwrap());
    assert_eq!(lane.states(), [(job, "in flight")]);
    eprintln!("FJ-stop: admitted after the ACK: {:?}", s_adm.journal());
}

/// F4-O (SIMULATED completion): F4-P's churn leaves candidate 1 prepared
/// internally, installed as one ordinary entry whose retained frame F is
/// queued in the fixture and claimed by the runtime. The declared
/// completion `present_queued_retained` takes exactly F out of the queue
/// and presents it (no exporter render, custody submit, DRM commit, flip
/// event, `custody.present`, cursor or capture: the fixture has no
/// device); `publish_presented_from` settles the claim keyed by (output,
/// F); the owner's `observe_presentation` then finds the presentation
/// epoch and journals FrameOutcome(2, None) for the exact candidate, which
/// ends the deferred run so the acceptance becomes visible, and the peer
/// reads both. Evidence of the publication response to a declared
/// completion of the exact queued fallback, not native completion
/// evidence.
#[test]
fn f4o_a_declared_completion_of_the_queued_fallback_journals_presented_and_settles_the_claim() {
    let (mut lane, wake) = Lane::running();
    let mut a = lane.attempt();
    let job = rendered(&mut lane, &wake, &mut a);
    let (key, n) = lane.admitted_key(job);
    assert!(lane.native.ordinary_queue_ready(output().id));
    assert_eq!(lane.native.last_retained_admission(), None);
    assert!(lane.runtime.queued_claims().is_empty());
    let (pp0, _) = peer_poll(&mut a);
    let s0 = lane.snapshot(&a, "F4-O O0");
    eprintln!("F4-O PP0 {pp0:?}; O0 journal {:?}", s0.journal());
    let (steps, observed) = lane.observed_turn(Instant::now(), MOVED);
    assert_eq!(steps, TURN);
    assert_eq!(observed, DECLINED, "declined before the walk");
    assert_eq!(lane.native.installs(), 1, "one ordinary install entry");
    assert_eq!(lane.native.ordinary_queues(), 1);
    let s1 = lane.snapshot(&a, "F4-O O1");
    let w1 = window(&s0, &s1, "F4-O T1");
    assert_eq!(kinds(&w1), ["ResourceStatus(2)"], "the acceptance alone");
    let status = one_status(&w1, a.key.grant, "F4-O T1");
    assert_eq!(
        s1.journal().deferred,
        Some((status.start, status.end)),
        "deferred"
    );
    assert_eq!(s1.facts.lean_frames_in_flight, Some(1));
    assert_eq!(
        lane.h.owner.processes.candidate_prepared(0, OUTPUT, n),
        Some(true)
    );
    assert_eq!(lane.pending(&a), [(a.key.grant, OUTPUT, 1)]);
    assert!(
        !lane.native.ordinary_queue_ready(output().id),
        "the fallback is queued, not ready"
    );
    let frame = *lane
        .native
        .last_retained_admission()
        .expect("one retained admission")
        .get(&output().id)
        .expect("for the lane's output");
    assert_eq!(
        lane.runtime.queued_claims(),
        [(
            output().id,
            frame,
            LiveShellContentLayer::Shell,
            a.key.grant
        )],
        "the runtime's claim on the exact queued frame"
    );
    assert_eq!(lane.native.presented_retained(output().id), None);
    // The declared completion.
    let presented = lane.native.present_queued_retained(output().id).unwrap();
    assert_eq!(presented, frame, "the exact queued fallback");
    assert_eq!(lane.native.presented_retained(output().id), Some(frame));
    assert!(
        lane.native.ordinary_queue_ready(output().id),
        "the queue no longer owns it"
    );
    assert_eq!(lane.native.installs(), 1);
    assert_eq!(lane.native.ordinary_queues(), 1);
    assert_eq!(lane.native.accepts(), 0);
    lane.runtime.publish_presented_from(&lane.native);
    assert!(
        lane.runtime.queued_claims().is_empty(),
        "the claim settled on (output, F)"
    );
    // The owner observes the presentation: one labelled transport pass.
    let runtime = &lane.runtime;
    let found = lane
        .h
        .owner
        .with_service(a.key, |service, t| {
            let ShellComponentService::Bar(bar) = service else {
                panic!("bar required")
            };
            let found = bar.observe_presentation(t, runtime).unwrap();
            t.poll_io().unwrap();
            found
        })
        .unwrap();
    assert!(
        found,
        "the presentation epoch was found for the installed frame"
    );
    let s2 = lane.snapshot(&a, "F4-O O2");
    let w2 = window(&s1, &s2, "F4-O OP");
    assert_eq!(kinds(&w2), ["FrameOutcome(2,0)"], "the Presented outcome");
    assert_eq!(peer_terminals(&frames_in(&w2), key, n), [PRESENTED]);
    assert_all_visible(s2.journal(), "F4-O O2");
    assert_eq!(s2.facts.lean_frames_in_flight, Some(0));
    assert!(lane.pending(&a).is_empty(), "the pending entry removed");
    s2.assert_stores_empty("F4-O O2");
    assert!(s2.facts.outbox.is_empty());
    peer_reads_terminal(&mut a, n, "F4-O");
    assert_eq!(a.terminals(n), [PRESENTED]);
    assert_eq!(
        peer_statuses(&a.contents, a.key.grant),
        [(2, status_of(status).unwrap().reason)],
        "the acceptance became visible with the terminal"
    );
    assert_eq!(a.accepted, 1);
    assert_eq!(lane.releases(), (1, 1));
    assert!(lane.states().is_empty());
    lane.slot_freed("F4-O");
}

/// BP: the real invalidation of a spare allocation, an owner call between
/// turns, transfers its AllocationResult to the outbox without a drain:
/// the identified nonempty pre-state, not blocked, the journal unchanged.
/// `stop` revokes the attempt and the persistent transport's outbox reads
/// empty afterwards through the slot-indexed observer (the wire is gone;
/// the outbox is the transport's own field, so the empty read is a read,
/// not a missing wire). The invalidation's own effects, the frame's
/// custody and the renderer lease are recorded separately; nothing about
/// source release or native retirement follows from the clear. The
/// budget origin is BP's own start: no ACK is withheld here.
#[test]
fn bp_a_spares_invalidation_leaves_the_outbox_nonempty_and_stop_clears_it() {
    let (mut lane, wake) = Lane::running();
    let mut a = lane.attempt();
    let mut fill = Fill {
        progress_0: Instant::now(),
        queued_0: 0,
        backing_0: 0,
        spares: Vec::new(),
    };
    grant_spare(&mut lane, &mut a, &mut fill, 2, 1, "BP spare");
    let spare = fill.spares[0];
    let job = rendered(&mut lane, &wake, &mut a);
    let (pp0, _) = peer_poll(&mut a);
    let s0 = lane.snapshot(&a, "BP O0");
    eprintln!("BP PP0 {pp0:?}; O0 journal {:?}", s0.journal());
    assert!(s0.facts.outbox.is_empty());
    let transaction = TransactionId::from_raw(INVALIDATE_BASE);
    lane.h
        .owner
        .with_service(a.key, |_, t| {
            t.invalidate_content_allocation(transaction, spare, ContentReason::Revoked)
        })
        .unwrap()
        .unwrap();
    let granted: Vec<ContentAllocationId> = lane
        .h
        .owner
        .with_service(a.key, |_, t| t.content_allocation_snapshots())
        .unwrap()
        .iter()
        .map(|snapshot| snapshot.allocation)
        .collect();
    let s_pre = lane.snapshot(&a, "BP pre-stop");
    assert_eq!(
        committed(s_pre.journal()),
        committed(s0.journal()),
        "the invalidation journals nothing itself"
    );
    assert_eq!(s_pre.facts.outbox.len(), 1, "{:?}", s_pre.facts.outbox);
    assert_eq!(
        outbox_result(&s_pre.facts.outbox[0], "BP pre-stop"),
        (transaction, spare),
        "the identified nonempty pre-state"
    );
    let OutboxRecordFacts::Content(_, ShellContentRecord::AllocationResult(result)) =
        &s_pre.facts.outbox[0].record
    else {
        unreachable!()
    };
    assert_eq!(result.reason, ContentReason::Revoked as u16);
    assert!(!s_pre.facts.output_blocked);
    assert!(s_pre.facts.wired);
    assert!(
        s_pre.stores.allocations.is_empty(),
        "transferred without a drain"
    );
    assert!(s_pre.releasing.is_empty(), "no SlotReleased was owed");
    assert_eq!(lane.states(), [(job, "in flight")]);
    assert_eq!(lane.leased(), 1);
    eprintln!(
        "BP pre-stop: allocations now {granted:?}; pending presentations {:?}",
        lane.pending(&a)
    );
    lane.h.owner.stop(a.key).unwrap();
    assert_eq!(
        lane.h.owner.phase(a.key).unwrap(),
        ComponentConnectionPhase::Revoked
    );
    let (attempt, facts) = lane
        .h
        .owner
        .processes
        .transport_facts(0)
        .expect("slot 0 persists after stop");
    assert_eq!(
        attempt,
        Some((a.key.grant, ComponentConnectionPhase::Revoked)),
        "the slot still names the revoked attempt"
    );
    assert!(!facts.wired, "the wire is gone");
    assert!(
        facts.outbox.is_empty(),
        "stop cleared the outbox: {:?}",
        facts.outbox
    );
    assert!(!facts.output_blocked);
    assert!(facts.journal.is_none(), "no wire, no journal facts");
    eprintln!(
        "BP post-stop: releasing {:?}, image pending {}",
        lane.h.owner.processes.owned_releasing(0),
        lane.h.owner.processes.image_pending(0)
    );
    assert_eq!(
        lane.states(),
        [(job, "in flight")],
        "stop collected nothing"
    );
    let (steps, accepted) = lane.quiet_driver_turn_at(Instant::now());
    assert_eq!(
        steps,
        [Images, Walk, Native],
        "the revoked slot skips content"
    );
    assert!(accepted.is_empty());
    assert_eq!(
        lane.releases(),
        (1, 1),
        "the walk released the ended grant's Held lease"
    );
    assert!(lane.states().is_empty());
    assert_eq!(lane.native.installs(), 0);
    lane.slot_freed("BP");
}
