//! W2 prototype: a candidate naming one predecessor to retire on
//! presentation, admitted only on a connection negotiated at revision 12
//! with capability bit 17 granted, and followed by `RetireOnPresent`
//! through the session's outcomes and releases.

#[allow(dead_code)] // Shared scripted peer; this file drives only part of it.
#[path = "support/file_wire_peer.rs"]
mod file_wire_peer;

use std::os::unix::net::UnixListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use file_wire_peer::{CONTENT_GRANT_EPOCH, EPOCH, Peer, Profile, WAIT};
use sophia_shell_client::*;
use sophia_shell_protocol::shell::content::{
    ContentCandidateRetire, SOPHIA_SHELL_CAPABILITY_CONTENT_RETIRE_ON_PRESENT as W2,
};
use sophia_shell_protocol::shell_files::*;
use sophia_shell_protocol::*;

const BAR: u64 =
    SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE;
const OUTPUT: ContentOutputId = ContentOutputId {
    id: 2,
    generation: 1,
};

fn grant() -> ContentGrant {
    ContentGrant {
        connection_epoch: EPOCH,
        content_grant_epoch: CONTENT_GRANT_EPOCH,
    }
}

fn tx(raw: u64) -> TransactionId {
    TransactionId::from_raw(raw)
}

fn resource(id: u64) -> ContentResourceId {
    ContentResourceId { id, generation: 1 }
}

fn intent(retire_transaction: u64, id: u64) -> ContentCandidateRetire {
    ContentCandidateRetire {
        retire_transaction: tx(retire_transaction),
        resource: resource(id),
    }
}

fn connect(revision: u16, capabilities: u64) -> (ShellConnection, Peer) {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!(
        "sophia-shell-w2-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path).unwrap();
    // A real host always adds the work-area reservation.
    let profile = Profile {
        role: "bar",
        revision,
        capabilities: capabilities | SOPHIA_SHELL_CAPABILITY_WORK_AREA_RESERVATION,
        limits_published: true,
    };
    let limits = ContentLimits::prototype(grant());
    let thread = std::thread::spawn(move || Peer::handshake_with_limits(listener, profile, limits));
    let connection = ShellConnection::connect_files(
        &path,
        ShellClientOptions {
            minimum_revision: revision,
            maximum_revision: revision,
            required_capabilities: capabilities,
            handshake_timeout: WAIT,
        },
    );
    let peer = thread.join().unwrap();
    let _ = std::fs::remove_file(&path);
    let mut connection = connection.unwrap();
    let mut peer = peer;
    let _ = content(&mut connection, &mut peer);
    (connection, peer)
}

/// Alternates the peer and the client until `take` yields, within [`WAIT`].
fn next<T>(
    connection: &mut ShellConnection,
    peer: &mut Peer,
    mut take: impl FnMut(&mut ShellConnection, &mut Peer) -> Option<T>,
) -> T {
    let deadline = Instant::now() + WAIT;
    loop {
        peer.pump();
        connection.poll_io().unwrap();
        if let Some(value) = take(connection, peer) {
            return value;
        }
        assert!(Instant::now() < deadline, "the exchange stalled");
    }
}

fn content(
    connection: &mut ShellConnection,
    peer: &mut Peer,
) -> (TransactionId, ShellContentRecord) {
    next(connection, peer, |connection, _| {
        connection.take_content().unwrap()
    })
}

fn only(admission: Admission) -> Ticket {
    let mut tickets = admission.tickets();
    let ticket = tickets.next().expect("one ticket");
    assert!(tickets.next().is_none(), "one record, one ticket");
    ticket
}

/// Carries the front submission to its `Submitted`, returning the record.
fn commit(connection: &mut ShellConnection, peer: &mut Peer, ticket: Ticket) -> Vec<u8> {
    let held = next(connection, peer, |_, peer| peer.submits.pop_front());
    let submit = decode_shell_file_submit(&held.data).unwrap();
    let record = peer.transactions.last().unwrap().clone();
    peer.answer_write(held.tag, SHELL_FILE_SUBMIT_BYTES as u32);
    let event = peer.submitted(submit.submission_id, ShellFileKind::Candidate);
    peer.push(event);
    next(connection, peer, |connection, _| {
        (connection.custody(ticket) == Some(Custody::Submitted)).then_some(())
    });
    record
}

/// One-placement candidate `generation` drawing resource `draws`.
fn records(generation: u64, draws: u64) -> Vec<ShellContentRecord> {
    vec![
        ShellContentRecord::CandidateBegin(ContentCandidateBegin {
            grant: grant(),
            output: OUTPUT,
            candidate_generation: generation,
            facts_generation: 1,
            pacing_permit: generation,
            interaction_generation: 1,
            surface_count: 1,
            placement_count: 1,
            target_count: 0,
        }),
        ShellContentRecord::CandidateChunk(ContentCandidateChunk {
            grant: grant(),
            candidate_generation: generation,
            chunk_ordinal: 0,
            surfaces: vec![ContentSurface {
                allocation: ContentAllocationId {
                    id: 1,
                    generation: 1,
                },
                scale_generation: 1,
                role: 1,
                edge: 1,
                margins: ContentMargins::default(),
                reservation_extent: 0,
                parent_surface_index: u16::MAX,
                anchor_parent_rect: ContentPixelRect::default(),
            }],
            placements: vec![ContentPlacement {
                resource: resource(draws),
                surface_index: 0,
                destination_x_px: 0,
                destination_y_px: 0,
            }],
            targets: vec![],
        }),
        ShellContentRecord::CandidateEnd(ContentCandidateEnd {
            grant: grant(),
            candidate_generation: generation,
            surface_count: 1,
            placement_count: 1,
            target_count: 0,
        }),
    ]
}

/// The whole candidate value `records` encodes.
fn value(transaction: u64, generation: u64, draws: u64) -> ShellFileCandidate {
    let [
        ShellContentRecord::CandidateBegin(begin),
        ShellContentRecord::CandidateChunk(chunk),
        _,
    ] = &records(generation, draws)[..]
    else {
        unreachable!()
    };
    ShellFileCandidate {
        transaction: tx(transaction),
        candidate: ContentCandidate {
            grant: grant(),
            candidate_generation: generation,
            output: OUTPUT,
            facts_generation: begin.facts_generation,
            pacing_permit: begin.pacing_permit,
            interaction_generation: begin.interaction_generation,
            surfaces: chunk.surfaces.clone(),
            placements: chunk.placements.clone(),
            targets: vec![],
        },
    }
}

fn header(submission_id: u64) -> ShellFileHeader {
    ShellFileHeader {
        kind: ShellFileKind::Candidate,
        connection_epoch: EPOCH,
        submission_id,
        sequence: 0,
    }
}

fn submission_id(record: &[u8]) -> u64 {
    decode_shell_file_record(record, ShellFileClass::Candidate)
        .unwrap()
        .header
        .submission_id
}

fn event(peer: &mut Peer, transaction: u64, record: ShellContentRecord) -> Vec<u8> {
    let value = ShellFileTransactionRecord {
        transaction: tx(transaction),
        record,
    };
    let (kind, body) = match &value.record {
        ShellContentRecord::ResourceReleased(_) => (
            ShellFileKind::ResourceReleased,
            encode_shell_file_resource_released_body(&value).unwrap(),
        ),
        _ => encode_shell_file_transaction_body(&value).unwrap(),
    };
    peer.event(kind, &body)
}

fn outcome(generation: u64, kind: u16) -> ShellContentRecord {
    ShellContentRecord::CandidateOutcome(ContentCandidateOutcome {
        grant: grant(),
        candidate_generation: generation,
        output: OUTPUT,
        kind,
        reason: 0,
        presentation_epoch: u64::from(kind == 2),
        work_area_generation: 0,
        wm_commit_generation: 0,
    })
}

fn released(id: u64, generation: u64) -> ShellContentRecord {
    ShellContentRecord::ResourceReleased(ContentResourceReleased {
        grant: grant(),
        resource: ContentResourceId { id, generation },
        reason: 0,
    })
}

#[test]
fn an_unnegotiated_connection_refuses_the_intent_before_any_queue_or_id_change() {
    // r11 and bit 16 are the separate draw lane's; neither permits W2. A
    // request for bit 17 at any revision but 12 is refused at the handshake
    // (`both_handshakes_accept_only_the_exact_w2_welcome`).
    for (revision, capabilities) in [(10, BAR), (11, BAR), (12, BAR), (12, BAR | 1 << 16)] {
        let (mut connection, mut peer) = connect(revision, capabilities);
        assert!(!connection.retire_on_present(), "r{revision}");
        assert!(matches!(
            connection.enqueue_content_group_with_retire_tracked(
                tx(5),
                &records(2, 2),
                intent(50, 1)
            ),
            Err(ShellClientError::MissingCapability)
        ));
        let ticket = only(
            connection
                .enqueue_content_group_tracked(tx(5), &records(2, 2))
                .unwrap(),
        );
        let record = commit(&mut connection, &mut peer, ticket);
        assert_eq!(submission_id(&record), 2, "no submission ID was spent");
        assert_eq!(
            record,
            encode_shell_file_candidate(header(2), &value(5, 2, 2)).unwrap()
        );
        assert_eq!(peer.transactions.len(), 1);
    }
}

#[test]
fn a_negotiated_intent_is_one_record_its_helper_sizes_and_plain_bytes_are_unchanged() {
    let (mut connection, mut peer) = connect(12, BAR | W2);
    assert!(connection.retire_on_present());
    let plain = only(
        connection
            .enqueue_content_group_tracked(tx(4), &records(1, 1))
            .unwrap(),
    );
    let plain = commit(&mut connection, &mut peer, plain);
    assert_eq!(
        plain,
        encode_shell_file_candidate(header(2), &value(4, 1, 1)).unwrap()
    );
    let flagged = only(
        connection
            .enqueue_content_group_with_retire_tracked(tx(5), &records(2, 2), intent(50, 1))
            .unwrap(),
    );
    let flagged = commit(&mut connection, &mut peer, flagged);
    let expected =
        encode_shell_file_candidate_with_retire(header(3), &value(5, 2, 2), &intent(50, 1))
            .unwrap();
    assert_eq!(flagged, expected);
    assert_eq!(
        shell_file_candidate_with_retire_bytes(header(3), &value(5, 2, 2), &intent(50, 1)),
        Ok(flagged.len())
    );
    assert_eq!(
        shell_file_candidate_bytes(header(3), &value(5, 2, 2)),
        Ok(flagged.len() - 24)
    );
    assert_eq!(
        decode_shell_file_candidate_with_retire(&flagged, true).unwrap(),
        (value(5, 2, 2), Some(intent(50, 1)))
    );
    assert!(decode_shell_file_candidate(&flagged).is_err());
}

#[test]
fn the_size_helper_refuses_what_the_encoder_refuses() {
    let value = value(5, 2, 2);
    for invalid in [
        intent(0, 1),
        ContentCandidateRetire {
            resource: ContentResourceId {
                id: 0,
                generation: 1,
            },
            ..intent(50, 1)
        },
        ContentCandidateRetire {
            resource: ContentResourceId {
                id: 1,
                generation: 0,
            },
            ..intent(50, 1)
        },
    ] {
        assert!(shell_file_candidate_with_retire_bytes(header(1), &value, &invalid).is_err());
        assert!(encode_shell_file_candidate_with_retire(header(1), &value, &invalid).is_err());
    }
    let mut wrong = header(1);
    wrong.kind = ShellFileKind::FrameDemand;
    assert!(shell_file_candidate_with_retire_bytes(wrong, &value, &intent(50, 1)).is_err());
}

#[test]
fn an_invalid_intent_is_refused_without_spending_an_id() {
    let (mut connection, mut peer) = connect(12, BAR | W2);
    for (transaction, retire, draws) in [
        (5, intent(0, 1), 2),
        (5, intent(5, 1), 2), // its own transaction
        (
            5,
            ContentCandidateRetire {
                resource: resource(0),
                ..intent(50, 1)
            },
            2,
        ),
        (
            5,
            ContentCandidateRetire {
                resource: ContentResourceId {
                    id: 1,
                    generation: 0,
                },
                ..intent(50, 1)
            },
            2,
        ),
        (5, intent(50, 2), 2), // the candidate draws what it retires
    ] {
        assert!(matches!(
            connection.enqueue_content_group_with_retire_tracked(
                tx(transaction),
                &records(2, draws),
                retire
            ),
            Err(ShellClientError::InvalidArgument(_))
        ));
    }
    let ticket = only(
        connection
            .enqueue_content_group_with_retire_tracked(tx(5), &records(2, 2), intent(50, 1))
            .unwrap(),
    );
    let record = commit(&mut connection, &mut peer, ticket);
    assert_eq!(submission_id(&record), 2, "the refusals spent no ID");
}

#[test]
fn the_queue_charges_the_intent_and_a_refusal_leaves_nothing_queued() {
    let (mut connection, mut peer) = connect(12, BAR | W2);
    let plain = shell_file_candidate_bytes(header(1), &value(5, 2, 2)).unwrap();
    let limits = connection.queue_limits();
    connection
        .set_queue_limits(QueueLimits {
            bytes: plain + 23,
            control_records: 0,
            control_bytes: 0,
            ..limits
        })
        .unwrap();
    assert!(matches!(
        connection.enqueue_content_group_with_retire_tracked(tx(5), &records(2, 2), intent(50, 1)),
        Err(ShellClientError::QueueSaturated)
    ));
    // The caller still owns the candidate and its intent: retried with
    // room, the same intent is the one record sent.
    connection.set_queue_limits(limits).unwrap();
    let ticket = only(
        connection
            .enqueue_content_group_with_retire_tracked(tx(5), &records(2, 2), intent(50, 1))
            .unwrap(),
    );
    let record = commit(&mut connection, &mut peer, ticket);
    assert_eq!(
        decode_shell_file_candidate_with_retire(&record, true)
            .unwrap()
            .1,
        Some(intent(50, 1))
    );
    assert_eq!(peer.transactions.len(), 1);
}

#[test]
fn a_busy_submit_retries_the_same_record_with_its_intent() {
    let (mut connection, mut peer) = connect(12, BAR | W2);
    let ticket = only(
        connection
            .enqueue_content_group_with_retire_tracked(tx(5), &records(2, 2), intent(50, 1))
            .unwrap(),
    );
    let held = next(&mut connection, &mut peer, |_, peer| {
        peer.submits.pop_front()
    });
    let first = peer.transactions.last().unwrap().clone();
    peer.answer_error(held.tag, 11);
    let retried = commit(&mut connection, &mut peer, ticket);
    assert_eq!(retried, first, "the retry resends the identical record");
    assert_eq!(
        decode_shell_file_candidate_with_retire(&retried, true)
            .unwrap()
            .1,
        Some(intent(50, 1))
    );
}

#[test]
fn closing_drops_a_queued_intent_with_its_candidate() {
    let (mut connection, _peer) = connect(12, BAR | W2);
    let ticket = only(
        connection
            .enqueue_content_group_with_retire_tracked(tx(5), &records(2, 2), intent(50, 1))
            .unwrap(),
    );
    connection.close();
    assert_eq!(connection.custody(ticket), Some(Custody::DroppedUnsent));
    let mut retire = RetireOnPresent::new(grant());
    retire.name(OUTPUT, 2, intent(50, 1)).unwrap();
    retire.revoke();
    assert!(!retire.holds(resource(1)));
}

/// Admits and submits candidate 2 retiring resource 1 under tx 50.
fn submitted() -> (ShellConnection, Peer, RetireOnPresent) {
    let (mut connection, mut peer) = connect(12, BAR | W2);
    let ticket = only(
        connection
            .enqueue_content_group_with_retire_tracked(tx(5), &records(2, 2), intent(50, 1))
            .unwrap(),
    );
    let mut retire = RetireOnPresent::new(grant());
    retire.name(OUTPUT, 2, intent(50, 1)).unwrap();
    commit(&mut connection, &mut peer, ticket);
    (connection, peer, retire)
}

fn observe(
    connection: &mut ShellConnection,
    peer: &mut Peer,
    retire: &mut RetireOnPresent,
) -> Result<RetireProgress, RetireOnPresentError> {
    let (transaction, record) = content(connection, peer);
    retire.observe(transaction, &record)
}

#[test]
fn presented_and_released_in_one_read_settle_in_order() {
    let (mut connection, mut peer, mut retire) = submitted();
    let mut batch = event(&mut peer, 5, outcome(2, 1));
    batch.extend(event(&mut peer, 5, outcome(2, 2)));
    batch.extend(event(&mut peer, 50, released(1, 1)));
    peer.push(batch);
    peer.flush_events();
    connection.poll_io().unwrap();
    let c = &mut connection;
    assert_eq!(
        observe(c, &mut peer, &mut retire),
        Ok(RetireProgress::Unrelated)
    );
    assert!(retire.holds(resource(1)), "Prepared is not terminal");
    assert_eq!(
        observe(c, &mut peer, &mut retire),
        Ok(RetireProgress::Retiring(resource(1)))
    );
    assert!(retire.holds(resource(1)));
    assert_eq!(
        observe(c, &mut peer, &mut retire),
        Ok(RetireProgress::Released(resource(1)))
    );
    assert!(!retire.holds(resource(1)));
}

#[test]
fn a_delayed_release_keeps_the_predecessor_held_until_it_arrives() {
    let (mut connection, mut peer, mut retire) = submitted();
    let presented = event(&mut peer, 5, outcome(2, 2));
    peer.push(presented);
    assert_eq!(
        observe(&mut connection, &mut peer, &mut retire),
        Ok(RetireProgress::Retiring(resource(1)))
    );
    // An unrelated release, and a stale generation of the same id, free
    // nothing.
    let other = event(&mut peer, 9, released(3, 1));
    peer.push(other);
    assert_eq!(
        observe(&mut connection, &mut peer, &mut retire),
        Ok(RetireProgress::Unrelated)
    );
    assert!(retire.holds(resource(1)));
    let late = event(&mut peer, 50, released(1, 1));
    peer.push(late);
    assert_eq!(
        observe(&mut connection, &mut peer, &mut retire),
        Ok(RetireProgress::Released(resource(1)))
    );
    // The ID is free again: it may be named by a later candidate.
    retire.name(OUTPUT, 3, intent(50, 1)).unwrap();
}

#[test]
fn a_rejected_candidate_keeps_its_predecessor() {
    let (mut connection, mut peer, mut retire) = submitted();
    let rejected = event(&mut peer, 5, outcome(2, 3));
    peer.push(rejected);
    assert_eq!(
        observe(&mut connection, &mut peer, &mut retire),
        Ok(RetireProgress::Kept(resource(1)))
    );
    assert!(!retire.holds(resource(1)), "the client owns it again");
}

#[test]
fn duplicate_names_and_stale_or_contradicting_events_are_refused() {
    let mut retire = RetireOnPresent::new(grant());
    retire.name(OUTPUT, 2, intent(50, 1)).unwrap();
    assert_eq!(
        retire.name(OUTPUT, 2, intent(51, 3)),
        Err(RetireOnPresentError::Duplicate)
    );
    assert_eq!(
        retire.name(OUTPUT, 3, intent(51, 1)),
        Err(RetireOnPresentError::Duplicate)
    );
    assert_eq!(
        retire.name(OUTPUT, 3, intent(50, 3)),
        Err(RetireOnPresentError::Duplicate)
    );
    // A release before Presented contradicts the intent.
    assert_eq!(
        retire.observe(tx(50), &released(1, 1)),
        Err(RetireOnPresentError::Misattributed)
    );
    let stale_grant = ContentGrant {
        content_grant_epoch: CONTENT_GRANT_EPOCH + 1,
        ..grant()
    };
    let ShellContentRecord::CandidateOutcome(mut foreign) = outcome(2, 2) else {
        unreachable!()
    };
    foreign.grant = stale_grant;
    assert_eq!(
        retire.observe(tx(5), &ShellContentRecord::CandidateOutcome(foreign)),
        Err(RetireOnPresentError::WrongGrant)
    );
    let ShellContentRecord::CandidateOutcome(mut elsewhere) = outcome(2, 2) else {
        unreachable!()
    };
    elsewhere.output = ContentOutputId {
        id: 3,
        generation: 1,
    };
    assert_eq!(
        retire.observe(tx(5), &ShellContentRecord::CandidateOutcome(elsewhere)),
        Err(RetireOnPresentError::Misattributed)
    );
    assert_eq!(
        retire.observe(tx(5), &outcome(9, 2)),
        Ok(RetireProgress::Unrelated)
    );
    assert_eq!(
        retire.observe(tx(5), &outcome(2, 2)),
        Ok(RetireProgress::Retiring(resource(1)))
    );
    // Wrong generation, wrong transaction, or another resource under the
    // retire transaction: none is this predecessor's release.
    assert_eq!(
        retire.observe(tx(50), &released(1, 2)),
        Err(RetireOnPresentError::Misattributed)
    );
    assert_eq!(
        retire.observe(tx(51), &released(1, 1)),
        Err(RetireOnPresentError::Misattributed)
    );
    assert_eq!(
        retire.observe(tx(52), &released(1, 2)),
        Ok(RetireProgress::Unrelated)
    );
    assert!(retire.holds(resource(1)));
    assert_eq!(
        retire.observe(tx(50), &released(1, 1)),
        Ok(RetireProgress::Released(resource(1)))
    );
}

const OWNED: u64 = IMAGE_CAPABILITY_OWNED_RING;

fn options_for(revision: u16, required: u64) -> ShellClientOptions {
    ShellClientOptions {
        minimum_revision: revision,
        maximum_revision: revision,
        required_capabilities: required,
        handshake_timeout: WAIT,
    }
}

fn socket(name: &str) -> std::path::PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!(
        "sophia-shell-w2-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_file(&path);
    path
}

fn profile(revision: u16, capabilities: u64) -> Profile {
    Profile {
        role: "bar",
        revision,
        capabilities: capabilities | SOPHIA_SHELL_CAPABILITY_WORK_AREA_RESERVATION,
        limits_published: true,
    }
}

/// The blocking handshake's result against a welcome of `granted` at `revision`.
fn blocking(
    revision: u16,
    granted: u64,
    options: ShellClientOptions,
) -> Result<ShellConnection, ShellClientError> {
    let path = socket("blocking");
    let listener = UnixListener::bind(&path).unwrap();
    let profile = profile(revision, granted);
    let limits = ContentLimits::prototype(grant());
    let thread = std::thread::spawn(move || Peer::handshake_with_limits(listener, profile, limits));
    let result = ShellConnection::connect_files(&path, options);
    // A refused client closes its socket, so the scripted peer ends either way.
    let peer = thread.join();
    let _ = std::fs::remove_file(&path);
    if result.is_ok() {
        drop(peer.unwrap());
    }
    result
}

/// The incremental handshake's result against the same welcome.
fn incremental(
    revision: u16,
    granted: u64,
    options: ShellClientOptions,
) -> Result<ShellConnection, ShellClientError> {
    let path = socket("incremental");
    let listener = UnixListener::bind(&path).unwrap();
    let profile = profile(revision, granted);
    let (stop, done) = std::sync::mpsc::channel::<()>();
    let thread = std::thread::spawn(move || {
        let mut peer = Peer::fragmented_retry_handshake(listener, profile);
        let end = Instant::now() + WAIT;
        while done.try_recv().is_err() && Instant::now() < end {
            peer.pump();
            std::thread::yield_now();
        }
    });
    let mut connecting = Connecting::direct(
        std::os::unix::net::UnixStream::connect(&path).unwrap(),
        sophia_9p_client::pipeline::PipelineLimits::default(),
        options,
        0,
        2000,
    )
    .unwrap();
    let started = Instant::now();
    let result = loop {
        assert!(started.elapsed() < WAIT, "incremental startup stalled");
        if let Err(error) = connecting.dispatch(started.elapsed().as_millis() as u64, 1) {
            break Err(error);
        }
        if let Some(error) = connecting.error() {
            break Err(error.clone());
        }
        if connecting.ready() {
            break Ok(connecting.take_connection().unwrap());
        }
        std::thread::yield_now();
    };
    drop(connecting);
    let _ = stop.send(());
    let _ = thread.join();
    let _ = std::fs::remove_file(&path);
    result
}

#[test]
fn both_handshakes_accept_only_the_exact_w2_welcome() {
    let required = BAR | OWNED | W2;
    type Handshake = fn(u16, u64, ShellClientOptions) -> Result<ShellConnection, ShellClientError>;
    for handshake in [blocking as Handshake, incremental as Handshake] {
        let connection = handshake(12, required, options_for(12, required)).unwrap();
        assert!(connection.retire_on_present());
        assert_eq!(connection.welcome().selected_revision, 12);
        // A W2 request never takes another revision, even inside its range.
        let wide = ShellClientOptions {
            minimum_revision: 10,
            maximum_revision: 13,
            ..options_for(12, required)
        };
        for revision in [10, 11, 13] {
            assert_eq!(
                handshake(revision, required, wide).err(),
                Some(ShellClientError::UnsupportedRevision),
                "r{revision}"
            );
        }
        // Exactly what was required: no extra grant beside bit 17.
        let extra = required | SOPHIA_SHELL_CAPABILITY_CONTENT_DISCRETE_INPUT;
        assert_eq!(
            handshake(12, extra, options_for(12, required)).err(),
            Some(ShellClientError::MissingCapability)
        );
        // Bit 17 is never accepted unrequested, at any revision.
        for revision in [10, 12] {
            assert_eq!(
                handshake(
                    revision,
                    BAR | OWNED | W2,
                    options_for(revision, BAR | OWNED)
                )
                .err(),
                Some(ShellClientError::MissingCapability),
                "r{revision}"
            );
        }
        // Without bit 17 the r10 image welcome is accepted as before.
        let r10 = handshake(10, BAR | OWNED, options_for(10, BAR | OWNED)).unwrap();
        assert!(!r10.retire_on_present());
    }
}

fn image_config() -> ShellImageConfig {
    ShellImageConfig {
        endpoint_dir: std::env::temp_dir(),
        bounds: ProducerRoleBounds {
            max_resource_bytes: 1 << 20,
            max_width: 64,
            max_height: 64,
            max_admitted_outputs: 1,
            transfer_slots: 4,
        },
        transfer_timeout_ms: 1_000,
        startup_deadline_ms: 60_000,
        formats_capacity: 4096,
        ring_slots: 0,
        ring_capacity: 0,
        content_grant_epoch: CONTENT_GRANT_EPOCH,
    }
}

#[test]
fn allocated_shell_revisions_open_owned_ring_without_implying_retirement() {
    for (revision, granted, opens) in [
        (10, BAR | OWNED, true),
        (12, BAR | OWNED | W2, true),
        (12, BAR | OWNED, true),
        (11, BAR | OWNED, true),
        (13, BAR | OWNED, false),
    ] {
        let mut connection = blocking(revision, granted, options_for(revision, granted)).unwrap();
        let enabled = connection.enable_image(image_config(), Instant::now(), 0);
        assert_eq!(
            !matches!(enabled, Err(ShellClientError::MissingCapability)),
            opens,
            "r{revision} {granted:#x}: {enabled:?}"
        );
        let mut connection = blocking(revision, granted, options_for(revision, granted)).unwrap();
        let dma = image_config();
        // DMA needs its own bit as well, so it stays closed here either way.
        assert!(matches!(
            connection.enable_dma_image(dma, Instant::now(), 0),
            Err(ShellClientError::MissingCapability)
        ));
    }
}

#[test]
fn image_options_take_exactly_r12_for_w2() {
    let path = socket("options");
    for (minimum, maximum, required, valid) in [
        (10, 10, BAR | OWNED, true),
        (9, 12, BAR | OWNED, true),
        (12, 12, BAR | OWNED | W2, true),
        (10, 12, BAR | OWNED | W2, false),
        (12, 13, BAR | OWNED | W2, false),
        (12, 12, BAR | OWNED, true),
        (11, 11, BAR | OWNED, true),
        (13, 13, BAR | OWNED, false),
    ] {
        let options = ShellClientOptions {
            minimum_revision: minimum,
            maximum_revision: maximum,
            required_capabilities: required,
            handshake_timeout: WAIT,
        };
        let result = ShellConnection::connect_files_with_image(
            &path,
            options,
            image_config(),
            Instant::now(),
            0,
        );
        // Invalid options refuse before any socket; valid ones fail to connect.
        assert_eq!(
            !matches!(
                result,
                Err(ShellClientError::InvalidArgument("image adapter options"))
            ),
            valid,
            "[{minimum},{maximum}] {required:#x}"
        );
    }
}
