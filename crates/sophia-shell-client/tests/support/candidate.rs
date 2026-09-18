use super::*;
use crate::*;
use sophia_protocol::*;

#[path = "catalog.rs"]
mod catalog;

fn grant() -> ContentGrant {
    ContentGrant {
        connection_epoch: 7,
        content_grant_epoch: 9,
    }
}

fn connection() -> (ShellConnection, std::os::unix::net::UnixStream) {
    let (stream, peer) = std::os::unix::net::UnixStream::pair().unwrap();
    stream.set_nonblocking(true).unwrap();
    (
        ShellConnection {
            stream,
            welcome: ShellV1ServerWelcome {
                selected_revision: 6,
                connection_epoch: 7,
                capabilities: SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER
                    | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE,
                max_descriptors: 16,
                max_label_bytes: 128,
                max_pending_activations: 16,
            },
            input: Vec::new(),
            output: outbox::ClientOutbox::default(),
            inbox: std::collections::VecDeque::new(),
            peer_closed: false,
        },
        peer,
    )
}

fn withdrawal() -> [ShellContentRecord; 2] {
    [
        ShellContentRecord::CandidateBegin(ContentCandidateBegin {
            grant: grant(),
            output: ContentOutputId {
                id: 1,
                generation: 1,
            },
            candidate_generation: 1,
            facts_generation: 1,
            pacing_permit: 1,
            interaction_generation: 1,
            surface_count: 0,
            placement_count: 0,
            target_count: 0,
        }),
        ShellContentRecord::CandidateEnd(ContentCandidateEnd {
            grant: grant(),
            candidate_generation: 1,
            surface_count: 0,
            placement_count: 0,
            target_count: 0,
        }),
    ]
}

#[test]
fn full_outbox_does_not_consume_candidate_identity_and_retry_owns_exactly_one_group() {
    let (mut connection, _peer) = connection();
    let mut lifecycle = ContentLifecycle::new(ContentLimits::prototype(grant())).unwrap();
    for _ in 0..32 {
        connection.output.enqueue(vec![vec![1]], false).unwrap();
    }
    let tx = TransactionId::from_raw(3);
    assert_eq!(
        connection.enqueue_candidate(&mut lifecycle, tx, &withdrawal()),
        Err(ShellClientError::QueueSaturated)
    );
    while let Some(frame) = connection.output.front() {
        let len = frame.len();
        connection.output.written(len);
    }
    connection
        .enqueue_candidate(&mut lifecycle, tx, &withdrawal())
        .unwrap();
    for expected in withdrawal() {
        let bytes = connection.output.front().unwrap();
        assert_eq!(decode_shell_content_frame(bytes).unwrap(), (tx, expected));
        let len = bytes.len();
        connection.output.written(len);
    }
    assert!(connection.output.front().is_none());
    assert!(matches!(
        connection.enqueue_candidate(&mut lifecycle, tx, &withdrawal()),
        Err(ShellClientError::Lifecycle(
            ContentLifecycleError::InvalidCandidate
        ))
    ));
    assert!(
        connection.output.front().is_none(),
        "duplicate registration cannot emit another Begin"
    );
}

#[test]
fn metadata_refusal_after_fifo_reservation_still_sends_nothing() {
    let (mut connection, _peer) = connection();
    let mut wrong_limits = ContentLimits::prototype(grant());
    wrong_limits.grant.content_grant_epoch += 1;
    let mut wrong = ContentLifecycle::new(wrong_limits).unwrap();
    let tx = TransactionId::from_raw(3);
    assert_eq!(
        connection.enqueue_candidate(&mut wrong, tx, &withdrawal()),
        Err(ShellClientError::Lifecycle(
            ContentLifecycleError::WrongGrant
        ))
    );
    assert!(connection.output.front().is_none());
    let mut correct = ContentLifecycle::new(ContentLimits::prototype(grant())).unwrap();
    connection
        .enqueue_candidate(&mut correct, tx, &withdrawal())
        .unwrap();
    assert!(connection.output.front().is_some());
}

#[test]
fn candidate_group_cannot_register_different_begin_and_end_identities() {
    let (mut connection, _peer) = connection();
    let mut lifecycle = ContentLifecycle::new(ContentLimits::prototype(grant())).unwrap();
    let mut records = withdrawal();
    if let ShellContentRecord::CandidateEnd(end) = &mut records[1] {
        end.candidate_generation = 2;
    }
    assert!(matches!(
        connection.enqueue_candidate(&mut lifecycle, TransactionId::from_raw(3), &records),
        Err(ShellClientError::Lifecycle(
            ContentLifecycleError::InvalidCandidate
        ))
    ));
    assert!(connection.output.front().is_none());
    connection
        .enqueue_candidate(&mut lifecycle, TransactionId::from_raw(3), &withdrawal())
        .unwrap();
}
