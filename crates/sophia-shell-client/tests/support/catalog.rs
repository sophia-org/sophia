use super::*;

fn catalog_connection() -> (ShellConnection, std::os::unix::net::UnixStream) {
    let (mut connection, peer) = connection();
    connection.welcome.selected_revision = 8;
    connection.welcome.capabilities |= SOPHIA_SHELL_CAPABILITY_PERSISTENT_CATALOG;
    (connection, peer)
}
fn publication(count: u16, generation: u64) -> Vec<Vec<u8>> {
    let tx = TransactionId::from_raw(50);
    let catalog = ShellApplicationCatalog {
        connection_epoch: 7,
        generation,
        entries: (1..=count)
            .map(|slot| ShellApplicationDescriptor {
                slot,
                available: true,
                label: format!("app{slot}"),
                keywords: String::new(),
            })
            .collect(),
    };
    let mut frames = encode_shell_application_catalog(tx, &catalog).unwrap();
    let end = frames.pop().unwrap();
    for slot in 1..=count {
        frames.push(
            encode_shell_catalog_action_frame(
                tx,
                &ShellCatalogActionRecord::Identity(ShellCatalogIdentity {
                    connection_epoch: 7,
                    catalog_generation: generation,
                    slot,
                    identity: format!("registered:app{slot}"),
                }),
            )
            .unwrap(),
        );
    }
    frames.push(end);
    frames
}

#[test]
fn partial_catalog_never_publishes_and_complete_transaction_is_exact() {
    let (mut client, _peer) = catalog_connection();
    let mut inbox = CatalogInbox::new(7).unwrap();
    let mut frames = publication(1, 1);
    let end = frames.pop().unwrap();
    client.inbox.extend(frames);
    assert!(
        client
            .take_catalog_observation(&mut inbox)
            .unwrap()
            .is_none()
    );
    client.inbox.push_back(end);
    let Some(CatalogObservation::Catalog(tx, catalog)) =
        client.take_catalog_observation(&mut inbox).unwrap()
    else {
        panic!("no complete publication")
    };
    assert_eq!(tx, TransactionId::from_raw(50));
    assert_eq!(catalog.identities[&1], "registered:app1");
    client.inbox.extend(publication(1, 1));
    assert!(
        client.take_catalog_observation(&mut inbox).is_err(),
        "generation replay refused"
    );
}

#[test]
fn large_catalog_spans_bounded_visits_without_a_full_inbox_deadlock() {
    use std::io::Write as _;
    let (mut client, mut peer) = catalog_connection();
    peer.set_write_timeout(Some(std::time::Duration::from_secs(2)))
        .unwrap();
    let mut inbox = CatalogInbox::new(7).unwrap();
    let frames = publication(128, 4);
    for frame in &frames {
        peer.write_all(frame).unwrap();
    }
    let mut completed = 0;
    for _ in 0..16 {
        client.poll_io().unwrap();
        if let Some(CatalogObservation::Catalog(_, catalog)) =
            client.take_catalog_observation(&mut inbox).unwrap()
        {
            assert_eq!(catalog.identities.len(), 128);
            completed += 1;
        }
        assert!(client.inbox.is_empty());
    }
    assert_eq!(completed, 1);
}

#[test]
fn missing_identity_wrong_transaction_and_wrong_epoch_refuse() {
    for mode in 0..3 {
        let (mut client, _peer) = catalog_connection();
        let mut inbox = CatalogInbox::new(if mode == 2 { 8 } else { 7 }).unwrap();
        let mut frames = publication(1, 1);
        if mode == 0 {
            frames.remove(2);
        }
        if mode == 1 {
            frames[2] = encode_shell_catalog_action_frame(
                TransactionId::from_raw(51),
                &ShellCatalogActionRecord::Identity(ShellCatalogIdentity {
                    connection_epoch: 7,
                    catalog_generation: 1,
                    slot: 1,
                    identity: "registered:app1".into(),
                }),
            )
            .unwrap();
        }
        client.inbox.extend(frames);
        assert!(
            client.take_catalog_observation(&mut inbox).is_err(),
            "mode {mode}"
        );
    }
}

#[test]
fn catalog_candidate_saturation_retains_lifecycle_and_exact_wire_family() {
    let (mut client, _peer) = catalog_connection();
    let mut lifecycle = ContentLifecycle::new(ContentLimits::prototype(grant())).unwrap();
    let [
        ShellContentRecord::CandidateBegin(begin),
        ShellContentRecord::CandidateEnd(end),
    ] = withdrawal()
    else {
        unreachable!()
    };
    let begin = CatalogCandidateBegin {
        content: begin,
        catalog_generation: 9,
    };
    let tx = TransactionId::from_raw(3);
    for _ in 0..32 {
        client.output.enqueue(vec![vec![1]], false).unwrap();
    }
    assert_eq!(
        client.enqueue_catalog_candidate(&mut lifecycle, tx, &begin, &[], &end),
        Err(ShellClientError::QueueSaturated)
    );
    while let Some(frame) = client.output.front() {
        let len = frame.len();
        client.output.written(len);
    }
    client
        .enqueue_catalog_candidate(&mut lifecycle, tx, &begin, &[], &end)
        .unwrap();
    let front = client.output.front().unwrap();
    assert_eq!(
        decode_shell_catalog_action_frame(front).unwrap(),
        (tx, ShellCatalogActionRecord::CandidateBegin(begin.clone()))
    );
    let len = front.len();
    client.output.written(len);
    assert_eq!(
        decode_shell_content_frame(client.output.front().unwrap()).unwrap(),
        (tx, ShellContentRecord::CandidateEnd(end.clone()))
    );
    assert!(
        client
            .enqueue_catalog_candidate(&mut lifecycle, tx, &begin, &[], &end)
            .is_err()
    );
}

fn action() -> ContentAction {
    ContentAction {
        grant: grant(),
        output: ContentOutputId {
            id: 1,
            generation: 1,
        },
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

#[test]
fn catalog_response_is_atomic_and_checks_complete_echo() {
    let (mut client, _peer) = catalog_connection();
    let activation = CatalogActivation {
        action: action(),
        catalog_generation: 3,
    };
    let mut wrong = ack(&activation.action);
    wrong.presentation_epoch += 1;
    let tx = TransactionId::from_raw(1);
    assert!(
        client
            .enqueue_catalog_action_response(tx, &wrong, Some((tx, &activation)))
            .is_err()
    );
    assert!(client.output.front().is_none());
    for _ in 0..63 {
        client.output.enqueue(vec![vec![1]], true).unwrap();
    }
    assert_eq!(
        client.enqueue_catalog_action_response(
            tx,
            &ack(&activation.action),
            Some((tx, &activation))
        ),
        Err(ShellClientError::QueueSaturated)
    );
    while let Some(frame) = client.output.front() {
        let len = frame.len();
        client.output.written(len);
    }
    client
        .enqueue_catalog_action_response(tx, &ack(&activation.action), Some((tx, &activation)))
        .unwrap();
    assert_eq!(
        decode_shell_content_frame(client.output.front().unwrap())
            .unwrap()
            .1,
        ShellContentRecord::ActionAck(ack(&activation.action))
    );
    let len = client.output.front().unwrap().len();
    client.output.written(len);
    assert_eq!(
        decode_shell_catalog_action_frame(client.output.front().unwrap())
            .unwrap()
            .1,
        ShellCatalogActionRecord::Activate(activation)
    );
}

#[test]
fn content_and_action_records_keep_wire_order_amid_catalog_assembly() {
    let (mut client, _peer) = catalog_connection();
    let mut inbox = CatalogInbox::new(7).unwrap();
    let tx = TransactionId::from_raw(1);
    let facts = ShellContentRecord::Limits(ContentLimits::prototype(grant()));
    client
        .inbox
        .push_back(encode_shell_content_frame(TransactionId::from_raw(0), &facts).unwrap());
    client.inbox.extend(publication(1, 1));
    client
        .inbox
        .push_back(encode_shell_content_frame(tx, &ShellContentRecord::Action(action())).unwrap());
    assert!(matches!(
        client.take_catalog_observation(&mut inbox).unwrap(),
        Some(CatalogObservation::Content(
            _,
            ShellContentRecord::Limits(_)
        ))
    ));
    assert!(matches!(
        client.take_catalog_observation(&mut inbox).unwrap(),
        Some(CatalogObservation::Catalog(_, _))
    ));
    assert!(matches!(
        client.take_catalog_observation(&mut inbox).unwrap(),
        Some(CatalogObservation::Content(
            _,
            ShellContentRecord::Action(_)
        ))
    ));
    client.welcome.selected_revision = 7;
    assert!(client.take_catalog_observation(&mut inbox).is_err());
}
