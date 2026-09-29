//! The file wire ("B6c") against a scripted `sophia_shell_fs_v1` peer: no
//! server core, only 9P2000.L framing and the file contract
//! (spec/sophia-shell-files.md; custody rules from spec/sophia-wm-files.md).
//!
//! Each test connects through `ShellConnection::connect_files`, then runs
//! the peer in lockstep with `poll_io` (see `support/file_wire_peer.rs`), so
//! it decides exactly which replies each pass sees, and asserts custody
//! through the tickets and the connection's state.

#[path = "support/file_wire_peer.rs"]
mod file_wire_peer;

use std::os::unix::net::UnixListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use file_wire_peer::{
    Ack, CONTENT_GRANT_EPOCH, EPOCH, Held, Logged, MSIZE, Object, Peer, Profile, WAIT,
};
use sophia_shell_client::*;
use sophia_shell_protocol::shell_files::*;
use sophia_shell_protocol::*;

const EAGAIN: u32 = 11;
#[path = "support/file_wire_admission.rs"]
mod admission;
const EINVAL: u32 = 22;
const EALREADY: u32 = 114;
/// The first `EAGAIN` backoff the client documents (`RETRY_FIRST`): a pass
/// shorter than this cannot have reached the retry by time alone.
const RETRY_FIRST: Duration = Duration::from_millis(2);
/// What one `events` or object read asks for: `msize - 11`.
const READ_COUNT: u32 = MSIZE - 11;

const APPLICATION_CATALOG: u64 = 1 << 5;
const BAR: u64 =
    SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE;
const DOCK: u64 = SOPHIA_SHELL_CAPABILITY_WORK_AREA_RESERVATION
    | APPLICATION_CATALOG
    | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
    | SOPHIA_SHELL_CAPABILITY_CONTENT_DISCRETE_INPUT
    | SOPHIA_SHELL_CAPABILITY_PERSISTENT_CATALOG;

// ---- connecting ----

/// The bar (r6): descriptor switcher and content surface, plus `extra`.
fn bar(extra: u64) -> Profile {
    Profile {
        role: "bar",
        revision: 6,
        capabilities: BAR | extra,
        limits_published: false,
    }
}

/// The dock (r8): the exact persistent-catalog mask.
fn dock() -> Profile {
    Profile {
        role: "dock",
        revision: 8,
        capabilities: DOCK,
        limits_published: false,
    }
}

fn options(profile: &Profile) -> ShellClientOptions {
    let (minimum_revision, required_capabilities) = if profile.revision == 8 {
        (8, DOCK)
    } else {
        (5, BAR)
    };
    ShellClientOptions {
        minimum_revision,
        maximum_revision: profile.revision,
        required_capabilities,
        handshake_timeout: WAIT,
    }
}

fn socket_path() -> std::path::PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    std::env::temp_dir().join(format!(
        "sophia-shell-files-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

/// Connects through the whole scripted handshake, then runs until the
/// client holds its one `events` read. The handshake's logs are cleared.
fn connect(profile: Profile) -> (ShellConnection, Peer) {
    let path = socket_path();
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path).unwrap();
    let thread = std::thread::spawn(move || Peer::handshake(listener, profile));
    let connection = ShellConnection::connect_files(&path, options(&profile));
    let mut peer = thread.join().unwrap();
    let _ = std::fs::remove_file(&path);
    let mut connection = connection.unwrap();
    assert_eq!(
        peer.acks.iter().map(|ack| ack.sequence).collect::<Vec<_>>(),
        [1, 2],
        "Submitted and Negotiated acknowledged"
    );
    peer.reset_logs();
    drive(&mut connection, &mut peer, |_, peer| {
        peer.events_read.is_some()
    })
    .unwrap();
    (connection, peer)
}

// ---- driving ----

/// Alternates the peer and `poll_io` until `done`, within [`WAIT`].
fn drive(
    connection: &mut ShellConnection,
    peer: &mut Peer,
    mut done: impl FnMut(&ShellConnection, &Peer) -> bool,
) -> Result<(), ShellClientError> {
    let deadline = Instant::now() + WAIT;
    loop {
        peer.pump();
        if done(connection, peer) {
            return Ok(());
        }
        assert!(Instant::now() < deadline, "the scripted exchange stalled");
        connection.poll_io()?;
    }
}

/// Alternates the peer and `poll_io` until `take` yields a value.
fn next<T>(
    connection: &mut ShellConnection,
    peer: &mut Peer,
    mut take: impl FnMut(&mut ShellConnection) -> Result<Option<T>, ShellClientError>,
) -> T {
    let deadline = Instant::now() + WAIT;
    loop {
        peer.pump();
        connection.poll_io().unwrap();
        if let Some(value) = take(connection).unwrap() {
            return value;
        }
        assert!(Instant::now() < deadline, "the scripted exchange stalled");
    }
}

/// Alternates the peer and `poll_io` until `poll_io` fails.
fn fail(connection: &mut ShellConnection, peer: &mut Peer) -> ShellClientError {
    let deadline = Instant::now() + WAIT;
    loop {
        peer.pump();
        if let Err(error) = connection.poll_io() {
            return error;
        }
        assert!(Instant::now() < deadline, "the connection never failed");
    }
}

/// Polls until `ticket` leaves `Queued`/`InFlight`, returning its custody
/// and the first error `poll_io` reported on the way.
fn settle(
    connection: &mut ShellConnection,
    peer: &mut Peer,
    ticket: Ticket,
) -> (Custody, Option<ShellClientError>) {
    let deadline = Instant::now() + WAIT;
    let mut first_error = None;
    loop {
        peer.pump();
        if let Err(error) = connection.poll_io() {
            first_error.get_or_insert(error);
        }
        match connection.custody(ticket) {
            Some(Custody::Queued | Custody::InFlight) => {}
            Some(custody) => return (custody, first_error),
            None => panic!("ticket {ticket:?} evicted"),
        }
        assert!(Instant::now() < deadline, "custody never settled");
    }
}

/// Runs until the client's next `submit` write waits at the peer.
fn submit(connection: &mut ShellConnection, peer: &mut Peer) -> Held {
    drive(connection, peer, |_, peer| !peer.submits.is_empty()).unwrap();
    peer.submits.pop_front().unwrap()
}

/// The submission ID and record kind a held submit names, checked against
/// the record staged in `transaction` just before it.
fn staged(peer: &Peer, held: &Held) -> (u64, ShellFileKind) {
    let submit = decode_shell_file_submit(&held.data).unwrap();
    assert_eq!(submit.connection_epoch, EPOCH);
    let record = peer.transactions.last().expect("a staged record");
    assert_eq!(
        record.len(),
        submit.candidate_bytes as usize,
        "exact length"
    );
    let header = decode_shell_file_record(record, ShellFileClass::Candidate)
        .unwrap()
        .header;
    assert_eq!(header.submission_id, submit.submission_id);
    (submit.submission_id, header.kind)
}

/// Carries the front unit through `transaction` and `submit` to its
/// `Submitted` event, returning the record the peer received.
fn commit(connection: &mut ShellConnection, peer: &mut Peer, ticket: Ticket) -> Vec<u8> {
    let held = submit(connection, peer);
    let (submission, kind) = staged(peer, &held);
    peer.answer_write(held.tag, SHELL_FILE_SUBMIT_BYTES as u32);
    let event = peer.submitted(submission, kind);
    peer.push(event);
    drive(connection, peer, |connection, _| {
        connection.custody(ticket) == Some(Custody::Submitted)
    })
    .unwrap();
    peer.transactions.last().unwrap().clone()
}

/// One `poll_io` pass after the peer refused a submit with `EAGAIN`.
/// Returns whether the submit already went again; asserts it did not
/// whenever the pass was too short for the first backoff to have passed.
fn pass_after_eagain(connection: &mut ShellConnection, peer: &mut Peer) -> bool {
    let started = Instant::now();
    connection.poll_io().unwrap();
    let elapsed = started.elapsed();
    peer.pump();
    let resent = !peer.submits.is_empty();
    if elapsed < RETRY_FIRST {
        assert!(!resent, "the submit went again in the pass that saw EAGAIN");
    }
    resent
}

// ---- records ----

fn tx(raw: u64) -> TransactionId {
    TransactionId::from_raw(raw)
}

fn grant() -> ContentGrant {
    ContentGrant {
        connection_epoch: EPOCH,
        content_grant_epoch: CONTENT_GRANT_EPOCH,
    }
}

fn output() -> ContentOutputId {
    ContentOutputId {
        id: 1,
        generation: 1,
    }
}

fn demand(demand_id: u64) -> ShellContentRecord {
    ShellContentRecord::FrameDemand(ContentFrameDemand {
        grant: grant(),
        output: output(),
        allocation: ContentAllocationId::default(),
        demand_id,
        reason: 1,
    })
}

fn enqueue_demand(connection: &mut ShellConnection, demand_id: u64) -> Ticket {
    let admission = connection
        .enqueue_content_tracked(tx(demand_id), &demand(demand_id))
        .unwrap();
    assert_eq!(admission.count, 1);
    admission.first
}

fn permit_record() -> ShellContentRecord {
    ShellContentRecord::FramePermit(ContentFramePermit {
        grant: grant(),
        output: output(),
        demand_id: 1,
        permit_id: 0,
        state: 3,
        reason: 11,
        ttl_ms: 0,
        max_candidate_bytes: 0,
    })
}

/// A cancelled `FramePermit` event: an ordinary event to consume.
fn permit(peer: &mut Peer) -> Vec<u8> {
    let (kind, body) = encode_shell_file_transaction_body(&ShellFileTransactionRecord {
        transaction: tx(70),
        record: permit_record(),
    })
    .unwrap();
    peer.event(kind, &body)
}

fn action() -> ContentAction {
    ContentAction {
        grant: grant(),
        output: output(),
        candidate_generation: 1,
        presentation_epoch: 1,
        interaction_generation: 1,
        allocation: ContentAllocationId {
            id: 1,
            generation: 1,
        },
        target_id: 1,
        target_generation: 1,
        action_id: 1,
        event_id: 1,
        kind: 1,
        reason: 0,
    }
}

fn ack(action: &ContentAction) -> ContentActionAck {
    ContentActionAck {
        grant: action.grant,
        output: action.output,
        candidate_generation: action.candidate_generation,
        presentation_epoch: action.presentation_epoch,
        interaction_generation: action.interaction_generation,
        allocation: action.allocation,
        target_id: action.target_id,
        target_generation: action.target_generation,
        action_id: action.action_id,
        event_id: action.event_id,
        disposition: 1,
    }
}

fn indicator_activation(event_id: u64) -> ShellIndicatorActivation {
    ShellIndicatorActivation {
        connection_epoch: EPOCH,
        snapshot_generation: 4,
        output: OutputId::from_raw(2),
        indicator: 3,
        action: 5,
        event_id,
    }
}

fn indicator_snapshot(generation: u64) -> ShellIndicatorSnapshot {
    ShellIndicatorSnapshot {
        connection_epoch: EPOCH,
        generation,
        active_output: Some(OutputId::from_raw(2)),
        statuses: vec![],
        indicators: vec![ShellIndicator {
            output: OutputId::from_raw(2),
            indicator: 3,
            action: 5,
            slot: 0,
            state_bits: 1,
            label: format!("view {generation}"),
        }],
    }
}

fn object_header(kind: ShellFileKind, epoch: u64) -> ShellFileHeader {
    ShellFileHeader {
        kind,
        connection_epoch: epoch,
        submission_id: 0,
        sequence: 0,
    }
}

fn indicators_object(epoch: u64, generation: u64) -> Vec<u8> {
    encode_shell_file_indicators(
        object_header(ShellFileKind::Indicators, epoch),
        &ShellFileIndicators {
            transaction: tx(60),
            snapshot: indicator_snapshot(generation),
        },
    )
    .unwrap()
}

fn publish(peer: &mut Peer, node: &str, qid: u64, bytes: Vec<u8>, chunk: usize) {
    peer.objects
        .insert(node.to_owned(), Object { qid, bytes, chunk });
}

/// A dock catalog of `count` entries, each with its identity.
fn catalog(count: u16, generation: u64) -> ShellPersistentCatalog {
    ShellPersistentCatalog {
        catalog: ShellApplicationCatalog {
            connection_epoch: EPOCH,
            generation,
            entries: (1..=count)
                .map(|slot| ShellApplicationDescriptor {
                    slot,
                    available: true,
                    label: format!("application {slot}"),
                    keywords: format!("keywords for {slot}"),
                })
                .collect(),
        },
        identities: (1..=count)
            .map(|slot| (slot, format!("registered:app{slot}")))
            .collect(),
    }
}

fn sequences(peer: &Peer) -> Vec<u64> {
    peer.acks.iter().map(|ack| ack.sequence).collect()
}

// ---- the handshake ----

#[test]
fn connect_negotiates_through_the_file_contract_and_reads_limits() {
    let profile = Profile {
        limits_published: true,
        ..bar(0)
    };
    let (mut connection, mut peer) = connect(profile);
    assert_eq!(connection.welcome(), profile.welcome());
    assert_eq!(connection.connection_epoch(), EPOCH);
    let hello = decode_shell_file_negotiate(peer.negotiate.as_ref().unwrap()).unwrap();
    assert_eq!(
        (
            hello.minimum_revision,
            hello.maximum_revision,
            hello.required_capabilities
        ),
        (5, 6, BAR)
    );
    let (_, record) = next(&mut connection, &mut peer, ShellConnection::take_content);
    let ShellContentRecord::Limits(limits) = record else {
        panic!("expected the limits object, got {record:?}");
    };
    assert_eq!(limits.grant, grant());
    // The first record after negotiation carries submission ID 2.
    let ticket = enqueue_demand(&mut connection, 1);
    let record = commit(&mut connection, &mut peer, ticket);
    let header = decode_shell_file_record(&record, ShellFileClass::Candidate)
        .unwrap()
        .header;
    assert_eq!(
        (header.kind, header.connection_epoch, header.submission_id),
        (ShellFileKind::FrameDemand, EPOCH, 2)
    );
}

// ---- 1. EAGAIN on submit ----

/// `EAGAIN` transferred nothing: the client keeps its staged record and
/// sends the same 24-byte submit again (same epoch and submission ID),
/// without rewriting `transaction`, once an event has been consumed -- not
/// in the pass that saw the refusal.
#[test]
fn an_eagain_submit_goes_again_unchanged_after_a_consumed_event() {
    let (mut connection, mut peer) = connect(bar(0));
    let ticket = enqueue_demand(&mut connection, 1);
    assert_eq!(connection.custody(ticket), Some(Custody::Queued));
    let refused = submit(&mut connection, &mut peer);
    assert_eq!(refused.data.len(), SHELL_FILE_SUBMIT_BYTES);
    assert_eq!(connection.custody(ticket), Some(Custody::InFlight));
    peer.answer_error(refused.tag, EAGAIN);
    if !pass_after_eagain(&mut connection, &mut peer) {
        assert!(connection.wake_deadline().is_some(), "a retry is scheduled");
        assert_eq!(connection.custody(ticket), Some(Custody::InFlight));
        let event = permit(&mut peer);
        peer.push(event);
    }
    let again = submit(&mut connection, &mut peer);
    assert_eq!(again.data, refused.data, "the same submit bytes");
    assert_eq!(peer.transactions.len(), 1, "transaction was not rewritten");
    assert_eq!(peer.walk_count("transaction"), 1, "nor reopened");
    let (submission, kind) = staged(&peer, &again);
    peer.answer_write(again.tag, SHELL_FILE_SUBMIT_BYTES as u32);
    let event = peer.submitted(submission, kind);
    peer.push(event);
    drive(&mut connection, &mut peer, |connection, _| {
        connection.custody(ticket) == Some(Custody::Submitted)
    })
    .unwrap();
    assert_eq!(connection.wake_deadline(), None, "nothing left to retry");
}

/// With no event to consume, the retry falls due at `wake_deadline()`.
#[test]
fn an_eagain_submit_goes_again_at_its_wake_deadline() {
    let (mut connection, mut peer) = connect(bar(0));
    let ticket = enqueue_demand(&mut connection, 1);
    let refused = submit(&mut connection, &mut peer);
    peer.answer_error(refused.tag, EAGAIN);
    if !pass_after_eagain(&mut connection, &mut peer) {
        let wake = connection.wake_deadline().expect("a retry is scheduled");
        assert!(wake <= Instant::now() + Duration::from_millis(64));
        let started = Instant::now();
        connection.wait_for_io(Duration::from_secs(1)).unwrap();
        assert!(started.elapsed() < Duration::from_millis(500));
        connection.poll_io().unwrap();
        peer.pump();
    }
    let again = peer.submits.pop_front().expect("the submit went again");
    assert_eq!(again.data, refused.data, "the same submit bytes");
    assert_eq!(peer.transactions.len(), 1, "transaction was not rewritten");
    let (submission, kind) = staged(&peer, &again);
    let event = peer.submitted(submission, kind);
    peer.push(event);
    peer.answer_write(again.tag, SHELL_FILE_SUBMIT_BYTES as u32);
    let (custody, error) = settle(&mut connection, &mut peer, ticket);
    assert_eq!((custody, error), (Custody::Submitted, None));
}

/// The refusal and an event reach the client in the same pass: the retry
/// still waits for a later pass.
#[test]
fn an_eagain_and_an_event_in_one_pass_do_not_resend_in_that_pass() {
    let (mut connection, mut peer) = connect(bar(0));
    enqueue_demand(&mut connection, 1);
    let refused = submit(&mut connection, &mut peer);
    peer.answer_error(refused.tag, EAGAIN);
    let event = permit(&mut peer);
    peer.push(event);
    peer.flush_events();
    let started = Instant::now();
    connection.poll_io().unwrap();
    let elapsed = started.elapsed();
    peer.pump();
    if elapsed < RETRY_FIRST {
        assert!(
            peer.submits.is_empty(),
            "the submit went again in the pass that saw EAGAIN"
        );
    }
}

// ---- 2. Submitted before the submit reply ----

#[test]
fn submitted_before_the_submit_reply_is_custody_and_the_unit_ends_at_the_reply() {
    let (mut connection, mut peer) = connect(bar(0));
    let first = enqueue_demand(&mut connection, 1);
    let second = enqueue_demand(&mut connection, 2);
    let held = submit(&mut connection, &mut peer);
    let (submission, kind) = staged(&peer, &held);
    let event = peer.submitted(submission, kind);
    peer.push(event);
    drive(&mut connection, &mut peer, |connection, _| {
        connection.custody(first) == Some(Custody::Submitted)
    })
    .unwrap();
    for _ in 0..4 {
        connection.poll_io().unwrap();
        peer.pump();
    }
    assert_eq!(
        peer.walk_count("transaction"),
        1,
        "the next unit waits for the submit reply"
    );
    assert_eq!(connection.custody(second), Some(Custody::Queued));
    peer.answer_write(held.tag, SHELL_FILE_SUBMIT_BYTES as u32);
    commit(&mut connection, &mut peer, second);
    assert_eq!(peer.walk_count("transaction"), 2);
    assert_eq!(connection.custody(first), Some(Custody::Submitted));
}

// ---- 3. the last read before end of file ----

/// A final `events` read carrying `Submitted`, then end of file: custody is
/// `Submitted`, whether or not the submit reply came first.
#[test]
fn a_final_submitted_before_end_of_file_is_custody() {
    for replied in [true, false] {
        let (mut connection, mut peer) = connect(bar(0));
        let ticket = enqueue_demand(&mut connection, 1);
        let held = submit(&mut connection, &mut peer);
        let (submission, kind) = staged(&peer, &held);
        if replied {
            peer.answer_write(held.tag, SHELL_FILE_SUBMIT_BYTES as u32);
            for _ in 0..2 {
                connection.poll_io().unwrap();
                peer.pump();
            }
            assert_eq!(connection.custody(ticket), Some(Custody::InFlight));
        }
        let event = peer.submitted(submission, kind);
        peer.push(event);
        peer.flush_events();
        peer.close();
        let (custody, _) = settle(&mut connection, &mut peer, ticket);
        assert_eq!(custody, Custody::Submitted, "replied first: {replied}");
        assert_eq!(
            connection.take_content(),
            Err(ShellClientError::PeerClosed),
            "replied first: {replied}"
        );
    }
}

/// A complete record that is not a valid event (API version 9).
fn malformed(peer: &Peer) -> Vec<u8> {
    let mut bytes = vec![0; SHELL_FILE_HEADER_BYTES];
    bytes[..4].copy_from_slice(&(SHELL_FILE_HEADER_BYTES as u32).to_le_bytes());
    bytes[4..6].copy_from_slice(&9u16.to_le_bytes());
    bytes[6..8].copy_from_slice(&(ShellFileKind::FramePermit as u16).to_le_bytes());
    bytes[8..16].copy_from_slice(&EPOCH.to_le_bytes());
    bytes[24..32].copy_from_slice(&peer.next_sequence().to_le_bytes());
    bytes
}

/// A malformed trailing record, then end of file: the unit whose
/// `Submitted` never arrived is `Unknown`; one whose `Submitted` came first
/// stays `Submitted`.
#[test]
fn a_malformed_trailing_record_before_end_of_file_leaves_unobserved_custody_unknown() {
    for observed in [false, true] {
        let (mut connection, mut peer) = connect(bar(0));
        let ticket = enqueue_demand(&mut connection, 1);
        let held = submit(&mut connection, &mut peer);
        let (submission, kind) = staged(&peer, &held);
        peer.answer_write(held.tag, SHELL_FILE_SUBMIT_BYTES as u32);
        if observed {
            let event = peer.submitted(submission, kind);
            peer.push(event);
        }
        let bad = malformed(&peer);
        peer.push(bad);
        peer.flush_events();
        peer.close();
        let (custody, error) = settle(&mut connection, &mut peer, ticket);
        let expected = if observed {
            Custody::Submitted
        } else {
            Custody::Unknown
        };
        assert_eq!(custody, expected, "Submitted observed: {observed}");
        if !observed {
            assert!(
                matches!(error, Some(ShellClientError::FileCodec(_))),
                "the malformed record fails the connection: {error:?}"
            );
        }
        // The record itself fails the connection either way.
        let mut error = error;
        let deadline = Instant::now() + WAIT;
        while error.is_none() {
            error = connection.poll_io().err();
            assert!(Instant::now() < deadline);
        }
        assert_eq!(connection.custody(ticket), Some(expected));
    }
}

/// The close scan applies normal intake's full decode: a record with a
/// valid envelope but a body intake would refuse (a malformed
/// `ResourceStatus`, an announcement for a feed not negotiated) ends the
/// scan, so a `Submitted` behind it settles nothing and custody is
/// `Unknown`. A valid event in the same place does not stop it. Each case
/// runs with the bad record first in line and behind an announcement whose
/// fetch holds later events back (the scan then looks past it).
#[test]
fn a_close_scan_stops_at_a_record_its_full_decode_refuses() {
    type Middle = fn(&mut Peer) -> Vec<u8>;
    let cases: [(&str, Middle, Custody); 3] = [
        (
            "malformed ResourceStatus body",
            |peer| peer.event(ShellFileKind::ResourceStatus, &[0; 8]),
            Custody::Unknown,
        ),
        (
            "announcement for a feed not negotiated",
            |peer| peer.published(ShellFileKind::Catalog, 1, 7),
            Custody::Unknown,
        ),
        ("a valid event", permit, Custody::Submitted),
    ];
    for fetch_ahead in [false, true] {
        for (case, middle, expected) in cases {
            let (mut connection, mut peer) = connect(bar(SOPHIA_SHELL_CAPABILITY_VIEW_INDICATORS));
            publish(
                &mut peer,
                "indicators",
                5,
                indicators_object(EPOCH, 1),
                usize::MAX,
            );
            peer.hold_object_reads = true;
            let ticket = enqueue_demand(&mut connection, 1);
            let held = submit(&mut connection, &mut peer);
            let (submission, kind) = staged(&peer, &held);
            peer.answer_write(held.tag, SHELL_FILE_SUBMIT_BYTES as u32);
            for _ in 0..2 {
                connection.poll_io().unwrap();
                peer.pump();
            }
            assert_eq!(connection.custody(ticket), Some(Custody::InFlight));
            if fetch_ahead {
                let event = peer.published(ShellFileKind::Indicators, 1, 5);
                peer.push(event);
            }
            let event = middle(&mut peer);
            peer.push(event);
            let event = peer.submitted(submission, kind);
            peer.push(event);
            peer.flush_events();
            peer.close();
            let (custody, _) = settle(&mut connection, &mut peer, ticket);
            assert_eq!(custody, expected, "{case}, fetch ahead: {fetch_ahead}");
        }
    }
}

/// One drain carries both the `events` read with this submission's
/// `Submitted` and the submit's `Rlerror`, the read first. `EALREADY` is
/// then custody and the lane continues; any other errno contradicts the
/// observed custody, which stays `Submitted` while the connection fails
/// closed. (A `Submitted` handled in an earlier pass is covered above.)
#[test]
fn submitted_and_a_submit_error_in_one_drain() {
    for errno in [EALREADY, EINVAL] {
        let (mut connection, mut peer) = connect(bar(0));
        let ticket = enqueue_demand(&mut connection, 1);
        let following = enqueue_demand(&mut connection, 2);
        let held = submit(&mut connection, &mut peer);
        let (submission, kind) = staged(&peer, &held);
        assert!(peer.events_read.is_some(), "the events read is outstanding");
        let event = peer.submitted(submission, kind);
        peer.push(event);
        peer.begin_batch();
        peer.flush_events();
        peer.answer_error(held.tag, errno);
        peer.end_batch();
        let result = connection.poll_io();
        assert_eq!(
            connection.custody(ticket),
            Some(Custody::Submitted),
            "errno {errno}"
        );
        if errno == EALREADY {
            assert_eq!(result, Ok(()), "EALREADY with Submitted continues");
            commit(&mut connection, &mut peer, following);
        } else {
            assert_eq!(
                result,
                Err(ShellClientError::Protocol("submit refused after Submitted"))
            );
            assert_eq!(connection.custody(following), Some(Custody::DroppedUnsent));
        }
    }
}

/// The same drain with the submit's `Rlerror` ahead of the `events` read:
/// the `Submitted` is in hand in that very drain, so custody is `Submitted`
/// either way. (Before the drain buffered events first, `EALREADY` settled
/// `Unknown` and `EINVAL` recorded `Refused(22)` for an accepted record.)
#[test]
fn a_submit_error_ahead_of_submitted_in_one_drain() {
    for errno in [EALREADY, EINVAL] {
        let (mut connection, mut peer) = connect(bar(0));
        let ticket = enqueue_demand(&mut connection, 1);
        let held = submit(&mut connection, &mut peer);
        let (submission, kind) = staged(&peer, &held);
        let event = peer.submitted(submission, kind);
        peer.push(event);
        peer.begin_batch();
        peer.answer_error(held.tag, errno);
        peer.flush_events();
        peer.end_batch();
        let _ = connection.poll_io();
        assert_eq!(
            connection.custody(ticket),
            Some(Custody::Submitted),
            "errno {errno}"
        );
    }
}

// ---- 4. disconnect ----

/// A disconnect after `submit` went out without `Submitted` is `Unknown`;
/// units still queued behind it never left the client.
#[test]
fn a_disconnect_after_submit_is_unknown_and_queued_units_are_dropped_unsent() {
    let (mut connection, mut peer) = connect(bar(0));
    let tickets = [1, 2, 3].map(|id| enqueue_demand(&mut connection, id));
    submit(&mut connection, &mut peer);
    peer.close();
    let (custody, _) = settle(&mut connection, &mut peer, tickets[0]);
    assert_eq!(custody, Custody::Unknown);
    assert_eq!(connection.custody(tickets[1]), Some(Custody::DroppedUnsent));
    assert_eq!(connection.custody(tickets[2]), Some(Custody::DroppedUnsent));
    assert_eq!(connection.take_content(), Err(ShellClientError::PeerClosed));
}

/// A disconnect while only the record is staged in `transaction` (its write
/// unanswered, or answered but `submit` not yet sent) transferred nothing.
#[test]
fn a_disconnect_with_only_the_transaction_written_is_dropped_unsent() {
    for answered in [false, true] {
        let (mut connection, mut peer) = connect(bar(0));
        peer.hold_transactions = true;
        let ticket = enqueue_demand(&mut connection, 1);
        drive(&mut connection, &mut peer, |_, peer| {
            !peer.held_transactions.is_empty()
        })
        .unwrap();
        if answered {
            let held = peer.held_transactions.pop_front().unwrap();
            peer.answer_write(held.tag, held.data.len() as u32);
        }
        peer.close();
        let (custody, _) = settle(&mut connection, &mut peer, ticket);
        assert_eq!(custody, Custody::DroppedUnsent, "answered: {answered}");
        assert!(peer.submits.is_empty(), "answered: {answered}");
    }
}

// ---- 5. EALREADY ----

#[test]
fn ealready_after_an_observed_submitted_is_custody() {
    let (mut connection, mut peer) = connect(bar(0));
    let ticket = enqueue_demand(&mut connection, 1);
    let held = submit(&mut connection, &mut peer);
    let (submission, kind) = staged(&peer, &held);
    let event = peer.submitted(submission, kind);
    peer.push(event);
    drive(&mut connection, &mut peer, |connection, _| {
        connection.custody(ticket) == Some(Custody::Submitted)
    })
    .unwrap();
    peer.answer_error(held.tag, EALREADY);
    let next_ticket = enqueue_demand(&mut connection, 2);
    commit(&mut connection, &mut peer, next_ticket);
    assert_eq!(connection.custody(ticket), Some(Custody::Submitted));
}

/// Without an observed `Submitted`, `EALREADY` leaves the outcome unknown
/// and the connection fails closed.
#[test]
fn ealready_without_submitted_is_unknown_and_fails_closed() {
    let (mut connection, mut peer) = connect(bar(0));
    let ticket = enqueue_demand(&mut connection, 1);
    let held = submit(&mut connection, &mut peer);
    peer.answer_error(held.tag, EALREADY);
    let expected = ShellClientError::Protocol("EALREADY without Submitted");
    assert_eq!(fail(&mut connection, &mut peer), expected);
    assert_eq!(connection.custody(ticket), Some(Custody::Unknown));
    assert_eq!(connection.poll_io(), Err(expected.clone()));
    assert_eq!(
        connection.enqueue_content_tracked(tx(2), &demand(2)),
        Err(expected)
    );
}

// ---- 6. other submit errors ----

/// Any other wire errno is the session's definitive refusal, carried as
/// sent; the lane moves on and the next record still reaches `Submitted`.
#[test]
fn another_submit_errno_is_a_refusal_and_the_lane_moves_on() {
    let (mut connection, mut peer) = connect(bar(0));
    for (demand_id, errno) in [(1, EINVAL), (3, 4242)] {
        let refused = enqueue_demand(&mut connection, demand_id);
        let following = enqueue_demand(&mut connection, demand_id + 1);
        let held = submit(&mut connection, &mut peer);
        peer.answer_error(held.tag, errno);
        let (custody, error) = settle(&mut connection, &mut peer, refused);
        assert_eq!((custody, error), (Custody::Refused(errno), None));
        commit(&mut connection, &mut peer, following);
    }
    assert_eq!(peer.walk_count("transaction"), 4, "one staging per record");
}

/// An error after `Submitted` was observed contradicts custody: the
/// connection fails closed and custody stays `Submitted`.
#[test]
fn a_submit_error_after_submitted_fails_closed_and_custody_stays() {
    let (mut connection, mut peer) = connect(bar(0));
    let ticket = enqueue_demand(&mut connection, 1);
    let held = submit(&mut connection, &mut peer);
    let (submission, kind) = staged(&peer, &held);
    let event = peer.submitted(submission, kind);
    peer.push(event);
    drive(&mut connection, &mut peer, |connection, _| {
        connection.custody(ticket) == Some(Custody::Submitted)
    })
    .unwrap();
    peer.answer_error(held.tag, EINVAL);
    assert_eq!(
        fail(&mut connection, &mut peer),
        ShellClientError::Protocol("submit refused after Submitted")
    );
    assert_eq!(connection.custody(ticket), Some(Custody::Submitted));
}

// ---- 7. short writes ----

#[test]
fn a_short_submit_write_is_a_protocol_error() {
    for count in [0, SHELL_FILE_SUBMIT_BYTES as u32 - 1] {
        let (mut connection, mut peer) = connect(bar(0));
        let ticket = enqueue_demand(&mut connection, 1);
        let held = submit(&mut connection, &mut peer);
        peer.answer_write(held.tag, count);
        assert_eq!(
            fail(&mut connection, &mut peer),
            ShellClientError::Protocol("short submit write"),
            "{count} of 24"
        );
        assert_eq!(connection.custody(ticket), Some(Custody::Unknown));
    }
}

#[test]
fn an_ack_write_of_another_count_is_a_protocol_error() {
    let (mut connection, mut peer) = connect(bar(0));
    peer.ack_reply = Some(SHELL_FILE_ACK_BYTES as u32 - 8);
    let event = permit(&mut peer);
    peer.push(event);
    assert_eq!(
        fail(&mut connection, &mut peer),
        ShellClientError::Protocol("unexpected ack reply")
    );
    assert_eq!(sequences(&peer), [3]);
}

// ---- 8. events that do not fit ----

#[test]
fn an_event_from_another_epoch_is_a_protocol_error() {
    let (mut connection, mut peer) = connect(bar(0));
    let (kind, body) = encode_shell_file_transaction_body(&ShellFileTransactionRecord {
        transaction: tx(70),
        record: permit_record(),
    })
    .unwrap();
    let event = peer.event_in(EPOCH + 1, kind, &body);
    peer.push(event);
    assert_eq!(
        fail(&mut connection, &mut peer),
        ShellClientError::Protocol("event from another connection epoch")
    );
    assert!(
        peer.acks.is_empty(),
        "the foreign event was not acknowledged"
    );
}

#[test]
fn a_submitted_naming_another_record_kind_is_a_protocol_error() {
    let (mut connection, mut peer) = connect(bar(0));
    let ticket = enqueue_demand(&mut connection, 1);
    let held = submit(&mut connection, &mut peer);
    let (submission, kind) = staged(&peer, &held);
    assert_eq!(kind, ShellFileKind::FrameDemand);
    let event = peer.submitted(submission, ShellFileKind::ActionAck);
    peer.push(event);
    assert_eq!(
        fail(&mut connection, &mut peer),
        ShellClientError::Protocol("unexpected Submitted event")
    );
    assert_eq!(connection.custody(ticket), Some(Custody::Unknown));
}

// ---- 9. snapshot objects ----

/// A catalog larger than `msize` is read across several reads; a positive
/// short read is not the end, a zero-length read is; and the announcement
/// is acknowledged only once its object is held.
#[test]
fn a_catalog_larger_than_msize_is_read_across_short_reads() {
    let (mut connection, mut peer) = connect(dock());
    let value = catalog(150, 3);
    let bytes = encode_shell_file_catalog(
        object_header(ShellFileKind::Catalog, EPOCH),
        &ShellFileCatalog {
            transaction: tx(50),
            catalog: value.clone(),
        },
    )
    .unwrap();
    assert!(bytes.len() > MSIZE as usize, "{} bytes", bytes.len());
    let total = bytes.len();
    publish(&mut peer, "catalog", 5, bytes, 8000);
    let sequence = peer.next_sequence();
    let event = peer.published(ShellFileKind::Catalog, 3, 5);
    peer.push(event);
    let mut inbox = CatalogInbox::new(EPOCH).unwrap();
    let observation = next(&mut connection, &mut peer, |connection| {
        connection.take_catalog_observation(&mut inbox)
    });
    let CatalogObservation::Catalog(transaction, delivered) = observation else {
        panic!("expected the catalog");
    };
    assert_eq!((transaction, delivered), (tx(50), value));

    let reads = peer.object_reads.clone();
    assert!(reads.len() > 2, "several reads: {reads:?}");
    let mut offset = 0;
    for read in &reads {
        assert_eq!((read.node.as_str(), read.offset), ("catalog", offset));
        assert_eq!(read.count, READ_COUNT, "a short read did not end it");
        offset += read.returned as u64;
    }
    assert_eq!(offset, total as u64);
    assert_eq!(reads.last().unwrap().returned, 0, "a zero read ended it");
    drive(&mut connection, &mut peer, |_, peer| !peer.acks.is_empty()).unwrap();
    assert_eq!(
        peer.acks,
        [Ack {
            sequence,
            after_reads: reads.len()
        }],
        "acknowledged only after the whole object"
    );
}

/// An object of exactly its feed's cap is followed by a one-byte probe; an
/// empty probe ends the object, which is then decoded (here, and only here,
/// failing on its deliberately empty body).
#[test]
fn an_object_exactly_at_its_cap_is_probed_with_one_byte() {
    let (mut connection, mut peer) = connect(bar(0));
    let bytes = encode_shell_file_record(
        object_header(ShellFileKind::Outputs, EPOCH),
        &[0; SHELL_FILE_OUTPUTS_MAX_BYTES - SHELL_FILE_HEADER_BYTES],
    )
    .unwrap();
    assert_eq!(bytes.len(), SHELL_FILE_OUTPUTS_MAX_BYTES);
    publish(&mut peer, "outputs", 5, bytes, usize::MAX);
    let event = peer.published(ShellFileKind::Outputs, 1, 5);
    peer.push(event);
    let error = fail(&mut connection, &mut peer);
    assert!(
        matches!(error, ShellClientError::FileCodec(_)),
        "the probe ended the object, which then failed to decode: {error:?}"
    );
    let cap = SHELL_FILE_OUTPUTS_MAX_BYTES;
    assert_eq!(
        peer.object_reads
            .iter()
            .map(|read| (read.offset, read.count, read.returned))
            .collect::<Vec<_>>(),
        [(0, READ_COUNT, cap), (cap as u64, 1, 0)]
    );
    assert!(peer.acks.is_empty());
}

#[test]
fn an_object_that_continues_past_its_cap_is_refused() {
    let (mut connection, mut peer) = connect(bar(0));
    let bytes = encode_shell_file_record(
        object_header(ShellFileKind::Outputs, EPOCH),
        &[0; SHELL_FILE_OUTPUTS_MAX_BYTES + 1 - SHELL_FILE_HEADER_BYTES],
    )
    .unwrap();
    publish(&mut peer, "outputs", 5, bytes, SHELL_FILE_OUTPUTS_MAX_BYTES);
    let event = peer.published(ShellFileKind::Outputs, 1, 5);
    peer.push(event);
    assert_eq!(
        fail(&mut connection, &mut peer),
        ShellClientError::Protocol("snapshot object over its cap")
    );
    let cap = SHELL_FILE_OUTPUTS_MAX_BYTES;
    assert_eq!(
        peer.object_reads
            .iter()
            .map(|read| (read.offset, read.count, read.returned))
            .collect::<Vec<_>>(),
        [(0, READ_COUNT, cap), (cap as u64, 1, 1)]
    );
    assert!(peer.acks.is_empty());
}

/// Opening pinned a newer object than the announcement: nothing is
/// delivered, and acknowledgement stays below the announcement even as
/// later events are handled, until the newer announcement is fetched.
#[test]
fn an_opened_object_newer_than_its_announcement_delivers_nothing_and_holds_the_ack() {
    let (mut connection, mut peer) = connect(bar(SOPHIA_SHELL_CAPABILITY_VIEW_INDICATORS));
    publish(
        &mut peer,
        "indicators",
        6,
        indicators_object(EPOCH, 2),
        usize::MAX,
    );
    let held_at = peer.next_sequence();
    let event = peer.published(ShellFileKind::Indicators, 1, 5);
    peer.push(event);
    let event = permit(&mut peer);
    peer.push(event);
    let (_, record) = next(&mut connection, &mut peer, ShellConnection::take_content);
    assert_eq!(record, permit_record());
    for _ in 0..4 {
        connection.poll_io().unwrap();
        peer.pump();
    }
    assert_eq!(peer.opens, [("indicators".to_owned(), 6)]);
    assert!(peer.object_reads.is_empty(), "the newer pin was not read");
    assert_eq!(connection.take_indicators(), Ok(None), "nothing delivered");
    assert!(
        peer.acks.iter().all(|ack| ack.sequence < held_at),
        "the ack passed an unfetched announcement: {:?}",
        peer.acks
    );

    let newest = peer.next_sequence();
    let event = peer.published(ShellFileKind::Indicators, 2, 6);
    peer.push(event);
    let (transaction, snapshot) =
        next(&mut connection, &mut peer, ShellConnection::take_indicators);
    assert_eq!((transaction, snapshot), (tx(60), indicator_snapshot(2)));
    drive(&mut connection, &mut peer, |_, peer| {
        peer.acks.iter().any(|ack| ack.sequence == newest)
    })
    .unwrap();
    assert_eq!(sequences(&peer), [newest]);
    assert_eq!(connection.take_indicators(), Ok(None), "one snapshot only");
}

#[test]
fn an_object_generation_other_than_announced_is_refused() {
    let (mut connection, mut peer) = connect(bar(SOPHIA_SHELL_CAPABILITY_VIEW_INDICATORS));
    publish(
        &mut peer,
        "indicators",
        5,
        indicators_object(EPOCH, 4),
        usize::MAX,
    );
    let event = peer.published(ShellFileKind::Indicators, 3, 5);
    peer.push(event);
    assert_eq!(
        fail(&mut connection, &mut peer),
        ShellClientError::Protocol("snapshot object generation differs from its announcement")
    );
    assert!(peer.acks.is_empty());
}

#[test]
fn an_object_from_another_epoch_or_feed_is_refused() {
    let foreign_epoch = indicators_object(EPOCH + 1, 1);
    let other_feed =
        encode_shell_file_record(object_header(ShellFileKind::Outputs, EPOCH), &[0; 64]).unwrap();
    for (case, bytes) in [("epoch", foreign_epoch), ("feed", other_feed)] {
        let (mut connection, mut peer) = connect(bar(SOPHIA_SHELL_CAPABILITY_VIEW_INDICATORS));
        publish(&mut peer, "indicators", 5, bytes, usize::MAX);
        let event = peer.published(ShellFileKind::Indicators, 1, 5);
        peer.push(event);
        assert_eq!(
            fail(&mut connection, &mut peer),
            ShellClientError::Protocol("snapshot object from another feed or connection epoch"),
            "{case}"
        );
        assert!(peer.acks.is_empty(), "{case}");
    }
}

#[test]
fn an_announcement_for_a_feed_not_negotiated_is_refused() {
    let (mut connection, mut peer) = connect(bar(0));
    publish(
        &mut peer,
        "indicators",
        5,
        indicators_object(EPOCH, 1),
        usize::MAX,
    );
    let event = peer.published(ShellFileKind::Indicators, 1, 5);
    peer.push(event);
    assert_eq!(
        fail(&mut connection, &mut peer),
        ShellClientError::Protocol("announced object kind")
    );
    assert!(peer.opens.is_empty(), "nothing was fetched");
    assert!(peer.acks.is_empty());
}

/// Two same-feed announcements buffered together: only the newer one is
/// fetched, and acknowledgement stays at the earlier one's bound until it
/// is.
#[test]
fn a_superseded_announcement_is_not_fetched_and_holds_the_ack_until_the_newer_is() {
    let (mut connection, mut peer) = connect(bar(SOPHIA_SHELL_CAPABILITY_VIEW_INDICATORS));
    publish(
        &mut peer,
        "indicators",
        6,
        indicators_object(EPOCH, 2),
        usize::MAX,
    );
    peer.hold_object_reads = true;
    let event = permit(&mut peer);
    peer.push(event);
    let older = peer.next_sequence();
    let event = peer.published(ShellFileKind::Indicators, 1, 5);
    peer.push(event);
    let newer = peer.next_sequence();
    let event = peer.published(ShellFileKind::Indicators, 2, 6);
    peer.push(event);
    peer.flush_events();
    drive(&mut connection, &mut peer, |_, peer| !peer.opens.is_empty()).unwrap();
    for _ in 0..4 {
        connection.poll_io().unwrap();
        peer.pump();
    }
    assert_eq!(
        peer.opens,
        [("indicators".to_owned(), 6)],
        "one fetch, the newer"
    );
    assert!(
        peer.acks.iter().all(|ack| ack.sequence < older),
        "the ack passed an unfetched announcement: {:?}",
        peer.acks
    );
    peer.release_object_reads();
    let (_, snapshot) = next(&mut connection, &mut peer, ShellConnection::take_indicators);
    assert_eq!(snapshot, indicator_snapshot(2));
    drive(&mut connection, &mut peer, |_, peer| {
        peer.acks.iter().any(|ack| ack.sequence == newer)
    })
    .unwrap();
    assert!(
        peer.acks
            .iter()
            .all(|ack| ack.sequence < older || ack.sequence == newer),
        "{:?}",
        peer.acks
    );
    assert_eq!(
        connection.take_indicators(),
        Ok(None),
        "the older one never"
    );
}

// ---- 10. role families ----

#[test]
fn indicator_and_catalog_outcome_events_decode_to_typed_outcomes() {
    let (mut connection, mut peer) = connect(bar(
        SOPHIA_SHELL_CAPABILITY_VIEW_INDICATORS | SOPHIA_SHELL_CAPABILITY_INDICATOR_ACTIVATION
    ));
    let outcome = ShellIndicatorActivationOutcome {
        connection_epoch: EPOCH,
        snapshot_generation: 4,
        event_id: 11,
        status: ShellIndicatorActivationStatus::Stale,
        reason: 1,
    };
    let body =
        encode_shell_file_indicator_activation_outcome_body(&ShellFileIndicatorActivationOutcome {
            transaction: tx(9),
            outcome,
        })
        .unwrap();
    let event = peer.event(ShellFileKind::IndicatorActivationOutcome, &body);
    peer.push(event);
    assert_eq!(
        next(
            &mut connection,
            &mut peer,
            ShellConnection::take_indicator_activation_outcome
        ),
        (tx(9), outcome)
    );

    let (mut connection, mut peer) = connect(dock());
    let outcome = CatalogActivationOutcome {
        activation: CatalogActivation {
            action: action(),
            catalog_generation: 3,
        },
        status: 2,
        reason: 0,
    };
    let (kind, body) = encode_shell_file_catalog_action_body(&ShellFileCatalogActionRecord {
        transaction: tx(12),
        record: ShellCatalogActionRecord::ActivationOutcome(outcome.clone()),
    })
    .unwrap();
    assert_eq!(kind, ShellFileKind::CatalogActivationOutcome);
    let event = peer.event(kind, &body);
    peer.push(event);
    let mut inbox = CatalogInbox::new(EPOCH).unwrap();
    let observation = next(&mut connection, &mut peer, |connection| {
        connection.take_catalog_observation(&mut inbox)
    });
    let CatalogObservation::Outcome(transaction, delivered) = observation else {
        panic!("expected the catalog activation outcome");
    };
    assert_eq!((transaction, delivered), (tx(12), outcome));
}

fn kind_and_id(record: &[u8]) -> (ShellFileKind, u64) {
    let header = decode_shell_file_record(record, ShellFileClass::Candidate)
        .unwrap()
        .header;
    (header.kind, header.submission_id)
}

/// An indicator activation is one `IndicatorActivate` record; an action
/// response is the ACK then the activation it authorizes, two tickets in
/// that order, each its own submission.
#[test]
fn indicator_activation_and_action_response_are_written_as_their_records_in_order() {
    let (mut connection, mut peer) = connect(bar(
        SOPHIA_SHELL_CAPABILITY_VIEW_INDICATORS | SOPHIA_SHELL_CAPABILITY_INDICATOR_ACTIVATION
    ));
    let alone = connection
        .enqueue_indicator_activation_tracked(tx(9), &indicator_activation(11))
        .unwrap();
    assert_eq!(alone.count, 1);
    let record = commit(&mut connection, &mut peer, alone.first);
    assert_eq!(kind_and_id(&record), (ShellFileKind::IndicatorActivate, 2));
    assert_eq!(
        decode_shell_file_indicator_activate(&record).unwrap(),
        ShellFileIndicatorActivate {
            transaction: tx(9),
            activation: indicator_activation(11),
        }
    );

    let activation = indicator_activation(12);
    let mut consumed = action();
    consumed.output.id = activation.output.raw();
    consumed.target_id = activation.indicator;
    consumed.action_id = activation.action;
    consumed.event_id = activation.event_id;
    let acknowledged = ack(&consumed);
    let pair = connection
        .enqueue_indicator_action_response_tracked(
            tx(10),
            &acknowledged,
            Some((tx(11), &activation)),
        )
        .unwrap();
    let tickets: Vec<Ticket> = pair.tickets().collect();
    assert_eq!(
        tickets,
        [Ticket(alone.first.0 + 1), Ticket(alone.first.0 + 2)]
    );
    let first = commit(&mut connection, &mut peer, tickets[0]);
    assert_eq!(kind_and_id(&first), (ShellFileKind::ActionAck, 3));
    assert_eq!(
        decode_shell_file_transaction(&first, ShellFileKind::ActionAck)
            .unwrap()
            .record,
        ShellContentRecord::ActionAck(acknowledged)
    );
    let second = commit(&mut connection, &mut peer, tickets[1]);
    assert_eq!(kind_and_id(&second), (ShellFileKind::IndicatorActivate, 4));
    assert_eq!(
        decode_shell_file_indicator_activate(&second)
            .unwrap()
            .activation,
        indicator_activation(12)
    );
}

/// A catalog candidate is one `CatalogCandidate` record; a catalog action
/// response is the ACK then the `CatalogActivate` it authorizes.
#[test]
fn catalog_candidate_and_action_response_are_written_as_their_records_in_order() {
    let (mut connection, mut peer) = connect(dock());
    let mut lifecycle = ContentLifecycle::new(ContentLimits::prototype(grant())).unwrap();
    let begin = CatalogCandidateBegin {
        content: ContentCandidateBegin {
            grant: grant(),
            output: output(),
            candidate_generation: 1,
            facts_generation: 1,
            pacing_permit: 1,
            interaction_generation: 1,
            surface_count: 0,
            placement_count: 0,
            target_count: 0,
        },
        catalog_generation: 9,
    };
    let end = ContentCandidateEnd {
        grant: grant(),
        candidate_generation: 1,
        surface_count: 0,
        placement_count: 0,
        target_count: 0,
    };
    let candidate = connection
        .enqueue_catalog_candidate_tracked(&mut lifecycle, tx(20), &begin, &[], &end)
        .unwrap();
    assert_eq!(candidate.count, 1, "one whole record");
    let record = commit(&mut connection, &mut peer, candidate.first);
    assert_eq!(kind_and_id(&record), (ShellFileKind::CatalogCandidate, 2));
    let decoded = decode_shell_file_catalog_candidate(&record).unwrap();
    assert_eq!(
        (decoded.transaction, decoded.candidate.catalog_generation),
        (tx(20), 9)
    );

    let activation = CatalogActivation {
        action: action(),
        catalog_generation: 9,
    };
    let acknowledged = ack(&action());
    let pair = connection
        .enqueue_catalog_action_response_tracked(tx(21), &acknowledged, Some((tx(22), &activation)))
        .unwrap();
    let tickets: Vec<Ticket> = pair.tickets().collect();
    assert_eq!(tickets.len(), 2);
    let first = commit(&mut connection, &mut peer, tickets[0]);
    assert_eq!(kind_and_id(&first), (ShellFileKind::ActionAck, 3));
    let second = commit(&mut connection, &mut peer, tickets[1]);
    assert_eq!(kind_and_id(&second), (ShellFileKind::CatalogActivate, 4));
    assert_eq!(
        decode_shell_file_catalog_action(&second, ShellFileKind::CatalogActivate).unwrap(),
        ShellFileCatalogActionRecord {
            transaction: tx(22),
            record: ShellCatalogActionRecord::Activate(activation),
        }
    );
}

// ---- the ack that releases `transaction` ----

/// The export keeps `transaction` busy until the previous `Submitted` is
/// acknowledged, and serves requests in order. With an earlier ack still
/// unanswered, unit N's `Submitted` is handled; its own ack must wait for
/// that reply. Then the ack reply (and anything else withheld, such as a
/// `transaction` walk, were one sent) reaches the client in one drain. The
/// ack covering N's `Submitted` must precede N+1's walk and open.
#[test]
fn the_next_record_waits_for_the_ack_covering_the_previous_submitted() {
    let (mut connection, mut peer) = connect(bar(0));
    let first = enqueue_demand(&mut connection, 1);
    let second = enqueue_demand(&mut connection, 2);
    let held = submit(&mut connection, &mut peer);
    let (submission, kind) = staged(&peer, &held);

    peer.hold_ack_replies = true;
    let event = permit(&mut peer);
    peer.push(event);
    drive(&mut connection, &mut peer, |_, peer| {
        !peer.held_replies.is_empty()
    })
    .unwrap();

    peer.hold_transaction_walks = true;
    let covering = peer.next_sequence();
    peer.answer_write(held.tag, SHELL_FILE_SUBMIT_BYTES as u32);
    let event = peer.submitted(submission, kind);
    peer.push(event);
    drive(&mut connection, &mut peer, |connection, _| {
        connection.custody(first) == Some(Custody::Submitted)
    })
    .unwrap();
    for _ in 0..4 {
        connection.poll_io().unwrap();
        peer.pump();
    }
    assert!(
        peer.acks.iter().all(|ack| ack.sequence < covering),
        "the covering ack went while an earlier one was unanswered"
    );
    assert_eq!(
        peer.walk_count("transaction"),
        1,
        "the next record started before the ack covering Submitted"
    );

    peer.begin_batch();
    peer.release_replies();
    peer.end_batch();
    commit(&mut connection, &mut peer, second);

    let covered = peer
        .log
        .iter()
        .position(|logged| matches!(logged, Logged::Ack(sequence) if *sequence >= covering))
        .expect("an ack covering Submitted");
    let walk = peer
        .log
        .iter()
        .rposition(|logged| *logged == Logged::Walk("transaction".to_owned()))
        .unwrap();
    let open = peer
        .log
        .iter()
        .rposition(|logged| *logged == Logged::Open("transaction".to_owned()))
        .unwrap();
    assert!(
        covered < walk && walk < open,
        "ack at {covered}, walk at {walk}, open at {open}: {:?}",
        peer.log
    );
}

/// An announcement whose object is not yet fetched holds acknowledgement
/// below it, so an ack covering a later `Submitted` cannot go; the next
/// record must not start until the fetch completes and that ack is queued.
#[test]
fn an_ack_held_by_an_unfetched_announcement_delays_the_next_record() {
    let (mut connection, mut peer) = connect(bar(SOPHIA_SHELL_CAPABILITY_VIEW_INDICATORS));
    publish(
        &mut peer,
        "indicators",
        6,
        indicators_object(EPOCH, 2),
        usize::MAX,
    );
    peer.hold_object_reads = true;
    let first = enqueue_demand(&mut connection, 1);
    let second = enqueue_demand(&mut connection, 2);
    let held = submit(&mut connection, &mut peer);
    let (submission, kind) = staged(&peer, &held);
    peer.answer_write(held.tag, SHELL_FILE_SUBMIT_BYTES as u32);
    // The older announcement is superseded, so it is never fetched, but it
    // holds the ack below itself until the newer one is.
    let event = peer.published(ShellFileKind::Indicators, 1, 5);
    peer.push(event);
    let covering = peer.next_sequence();
    let event = peer.submitted(submission, kind);
    peer.push(event);
    let event = peer.published(ShellFileKind::Indicators, 2, 6);
    peer.push(event);
    peer.flush_events();
    drive(&mut connection, &mut peer, |connection, _| {
        connection.custody(first) == Some(Custody::Submitted)
    })
    .unwrap();
    drive(&mut connection, &mut peer, |_, peer| !peer.opens.is_empty()).unwrap();
    for _ in 0..4 {
        connection.poll_io().unwrap();
        peer.pump();
    }
    assert!(
        peer.acks.iter().all(|ack| ack.sequence < covering),
        "{:?}",
        peer.acks
    );
    assert_eq!(
        peer.walk_count("transaction"),
        1,
        "the next record started while its ack was held back"
    );

    peer.release_object_reads();
    commit(&mut connection, &mut peer, second);
    let fetched = peer
        .log
        .iter()
        .position(|logged| {
            matches!(logged, Logged::ObjectRead { node, returned: 0 } if node == "indicators")
        })
        .expect("the fetch completed");
    let covered = peer
        .log
        .iter()
        .position(|logged| matches!(logged, Logged::Ack(sequence) if *sequence >= covering))
        .expect("an ack covering Submitted");
    let walk = peer
        .log
        .iter()
        .rposition(|logged| *logged == Logged::Walk("transaction".to_owned()))
        .unwrap();
    assert!(
        fetched < covered && covered < walk,
        "fetch at {fetched}, ack at {covered}, walk at {walk}: {:?}",
        peer.log
    );
}

// ---- pipeline pressure ----

/// A clunk the pipeline cannot queue fails the connection closed rather
/// than leaving a live fid behind. Every clunk that follows a reply reuses
/// the tag that reply just freed, so pressure can only bite on a clunk an
/// event causes: a terminal `ResourceStatus` closing its open slot writer.
/// Here the peer never answers a clunk, so 126 held clunks plus the
/// `events` read fill 127 of the 128 tags; the status waits behind an
/// object fetch, whose own clunk takes the last tag, and the writer's clunk
/// then finds none.
#[test]
fn a_clunk_the_pipeline_cannot_queue_fails_the_connection_closed() {
    let profile = Profile {
        limits_published: true,
        ..bar(SOPHIA_SHELL_CAPABILITY_VIEW_INDICATORS)
    };
    let (mut connection, mut peer) = connect(profile);
    let (_, limits) = next(&mut connection, &mut peer, ShellConnection::take_content);
    assert!(matches!(limits, ShellContentRecord::Limits(_)));
    publish(
        &mut peer,
        "indicators",
        5,
        indicators_object(EPOCH, 1),
        usize::MAX,
    );
    peer.hold_clunks = true;

    let resource = ContentResourceId {
        id: 1,
        generation: 1,
    };
    let begin = ShellContentRecord::ResourceBegin(ContentResourceBegin {
        grant: grant(),
        resource,
        width_px: 4,
        height_px: 4,
        rendered_scale_numerator: 1,
        rendered_scale_denominator: 1,
        pixel_format: 1,
        chunk_count: 1,
        total_bytes: 64,
    });
    let mut tickets = vec![
        connection
            .enqueue_content_tracked(tx(30), &begin)
            .unwrap()
            .first,
    ];
    commit(&mut connection, &mut peer, tickets[0]);
    let status = |peer: &mut Peer, status: u16| {
        let body = encode_shell_file_resource_status_body(&ShellFileTransactionRecord {
            transaction: tx(31),
            record: ShellContentRecord::ResourceStatus(ContentResourceStatus {
                grant: grant(),
                resource,
                status,
                reason: 0,
                next_ordinal: 0,
                admitted_bytes: 0,
            }),
        })
        .unwrap();
        peer.event(ShellFileKind::ResourceStatus, &body)
    };
    let admitted = status(&mut peer, 1);
    peer.push(admitted);
    drive(&mut connection, &mut peer, |_, peer| {
        peer.opens.iter().any(|(node, _)| node == "upload/0")
    })
    .unwrap();
    for id in 0..125 {
        let ticket = enqueue_demand(&mut connection, 100 + id);
        commit(&mut connection, &mut peer, ticket);
        tickets.push(ticket);
    }
    for _ in 0..4 {
        connection.poll_io().unwrap();
        peer.pump();
    }
    assert_eq!(peer.held_clunks, 126);
    let acked = peer.acks.last().map(|ack| ack.sequence);

    let event = peer.published(ShellFileKind::Indicators, 1, 5);
    peer.push(event);
    let terminal = status(&mut peer, 2);
    peer.push(terminal);
    assert_eq!(
        fail(&mut connection, &mut peer),
        ShellClientError::Pipeline(sophia_9p_client::pipeline::PipelineError::Limit(
            "max outstanding requests reached"
        ))
    );
    assert_eq!(
        peer.object_reads.last().map(|read| read.returned),
        Some(0),
        "the fetch finished; the writer's clunk is what failed"
    );
    assert_eq!(peer.acks.last().map(|ack| ack.sequence), acked);
    for ticket in &tickets {
        assert_eq!(connection.custody(*ticket), Some(Custody::Submitted));
    }
    assert!(connection.poll_io().is_err(), "failed closed");
    assert!(
        connection
            .enqueue_content_tracked(tx(1), &demand(1))
            .is_err()
    );
}

// ---- liveness ----

/// A request the client queues because a reply arrived (here the
/// `transaction` open after its walk) must reach the wire in that same
/// `poll_io` pass: nothing else would wake a caller that waits for the
/// socket to become readable, and the session has nothing to answer.
#[test]
fn a_request_queued_on_a_reply_is_written_in_the_same_pass() {
    let (mut connection, mut peer) = connect(bar(0));
    enqueue_demand(&mut connection, 1);
    connection.poll_io().unwrap();
    peer.pump();
    assert_eq!(peer.walk_count("transaction"), 1, "the walk went out");
    assert_eq!(connection.wake_deadline(), None);
    connection.poll_io().unwrap();
    peer.pump();
    assert!(
        peer.opens.iter().any(|(node, _)| node == "transaction"),
        "the open queued on the walk reply was left unwritten"
    );
}

#[test]
fn readiness_wait_blocks_when_idle_and_wakes_for_a_peer_event() {
    let (mut connection, mut peer) = connect(bar(0));
    // Drain the handshake's final plumbing; only the held events read remains.
    for _ in 0..8 {
        connection.poll_io().unwrap();
        peer.pump();
    }
    let started = Instant::now();
    connection.wait_for_io(Duration::from_millis(25)).unwrap();
    assert!(
        started.elapsed() >= Duration::from_millis(20),
        "idle writability spun"
    );
    let sender = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(30));
        let event = permit(&mut peer);
        peer.push(event);
        peer.flush_events();
        peer
    });
    let started = Instant::now();
    connection.wait_for_io(Duration::from_secs(2)).unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "readiness did not interrupt wait"
    );
    let mut peer = sender.join().unwrap();
    connection.poll_io().unwrap();
    peer.pump();
    connection.poll_io().unwrap();
    // A retained typed event needs application service, even with a quiet socket.
    let started = Instant::now();
    connection.wait_for_io(Duration::from_secs(2)).unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "buffered event slept"
    );
    assert_eq!(
        connection.take_content().unwrap().unwrap().1,
        permit_record()
    );
}

#[test]
fn readiness_wait_flushes_queued_work_and_wakes_on_disconnect() {
    let (mut connection, mut peer) = connect(bar(0));
    enqueue_demand(&mut connection, 1);
    let sender = std::thread::spawn(move || {
        let deadline = Instant::now() + WAIT;
        while peer.walk_count("transaction") == 0 {
            peer.pump();
            assert!(Instant::now() < deadline, "wait left queued work unsent");
            std::thread::yield_now();
        }
        peer.close();
    });
    let started = Instant::now();
    // Closure may be observed during the initial nonblocking pass too.
    let _ = connection.wait_for_io(Duration::from_secs(2));
    assert!(started.elapsed() < Duration::from_secs(1));
    sender.join().unwrap();
    // The first wake may be the walk reply; service the subsequently closed fd.
    assert!(
        connection.poll_io().is_err()
            || connection.take_content() == Err(ShellClientError::PeerClosed)
    );
}
