use super::*;

fn withdrawal() -> [ShellContentRecord; 2] {
    [
        ShellContentRecord::CandidateBegin(ContentCandidateBegin {
            grant: grant(),
            output: output(),
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

fn fill_bulk(connection: &mut ShellConnection) -> Vec<Ticket> {
    (1..=32).map(|id| enqueue_demand(connection, id)).collect()
}

#[test]
fn full_outbox_preserves_candidate_identity_for_one_retry() {
    let (mut connection, mut peer) = connect(bar(0));
    let mut lifecycle = ContentLifecycle::new(ContentLimits::prototype(grant())).unwrap();
    let pending = fill_bulk(&mut connection);
    assert_eq!(
        connection.enqueue_candidate_tracked(&mut lifecycle, tx(90), &withdrawal()),
        Err(ShellClientError::QueueSaturated)
    );
    for ticket in pending {
        commit(&mut connection, &mut peer, ticket);
    }
    let owned = connection
        .enqueue_candidate_tracked(&mut lifecycle, tx(90), &withdrawal())
        .unwrap();
    assert_eq!(owned.count, 1);
    let record = commit(&mut connection, &mut peer, owned.first);
    let decoded = decode_shell_file_candidate(&record).unwrap();
    assert_eq!(decoded.transaction, tx(90));
    assert_eq!(decoded.candidate.candidate_generation, 1);
    assert!(decoded.candidate.surfaces.is_empty());
    assert!(
        connection
            .enqueue_candidate_tracked(&mut lifecycle, tx(90), &withdrawal())
            .is_err()
    );
    let next = enqueue_demand(&mut connection, 91);
    let record = commit(&mut connection, &mut peer, next);
    assert_eq!(kind_and_id(&record).0, ShellFileKind::FrameDemand);
}

#[test]
fn metadata_refusal_after_reservation_leaves_no_candidate_or_ticket() {
    let (mut connection, mut peer) = connect(bar(0));
    let mut wrong_limits = ContentLimits::prototype(grant());
    wrong_limits.grant.content_grant_epoch += 1;
    let mut wrong = ContentLifecycle::new(wrong_limits).unwrap();
    assert_eq!(
        connection.enqueue_candidate_tracked(&mut wrong, tx(3), &withdrawal()),
        Err(ShellClientError::Lifecycle(
            ContentLifecycleError::WrongGrant
        ))
    );
    let mut correct = ContentLifecycle::new(ContentLimits::prototype(grant())).unwrap();
    let owned = connection
        .enqueue_candidate_tracked(&mut correct, tx(3), &withdrawal())
        .unwrap();
    assert_eq!(owned.first, Ticket(1));
    let record = commit(&mut connection, &mut peer, owned.first);
    assert_eq!(
        decode_shell_file_candidate(&record).unwrap().transaction,
        tx(3)
    );
}

#[test]
fn mismatched_candidate_parts_leave_identity_available() {
    let (mut connection, mut peer) = connect(bar(0));
    let mut lifecycle = ContentLifecycle::new(ContentLimits::prototype(grant())).unwrap();
    let mut records = withdrawal();
    if let ShellContentRecord::CandidateEnd(end) = &mut records[1] {
        end.candidate_generation = 2;
    }
    assert!(
        connection
            .enqueue_candidate_tracked(&mut lifecycle, tx(3), &records)
            .is_err()
    );
    let owned = connection
        .enqueue_candidate_tracked(&mut lifecycle, tx(3), &withdrawal())
        .unwrap();
    assert_eq!(owned.first, Ticket(1));
    let record = commit(&mut connection, &mut peer, owned.first);
    assert_eq!(
        decode_shell_file_candidate(&record)
            .unwrap()
            .candidate
            .candidate_generation,
        1
    );
}

#[test]
fn catalog_saturation_preserves_lifecycle_and_file_kind() {
    let (mut connection, mut peer) = connect(dock());
    let mut lifecycle = ContentLifecycle::new(ContentLimits::prototype(grant())).unwrap();
    let [
        ShellContentRecord::CandidateBegin(content),
        ShellContentRecord::CandidateEnd(end),
    ] = withdrawal()
    else {
        unreachable!()
    };
    let begin = CatalogCandidateBegin {
        content,
        catalog_generation: 9,
    };
    let pending = fill_bulk(&mut connection);
    assert_eq!(
        connection.enqueue_catalog_candidate_tracked(&mut lifecycle, tx(90), &begin, &[], &end),
        Err(ShellClientError::QueueSaturated)
    );
    for ticket in pending {
        commit(&mut connection, &mut peer, ticket);
    }
    let owned = connection
        .enqueue_catalog_candidate_tracked(&mut lifecycle, tx(90), &begin, &[], &end)
        .unwrap();
    assert_eq!(owned.count, 1);
    let record = commit(&mut connection, &mut peer, owned.first);
    let value = decode_shell_file_catalog_candidate(&record).unwrap();
    assert_eq!(value.transaction, tx(90));
    assert_eq!(value.candidate.catalog_generation, 9);
    assert!(
        connection
            .enqueue_catalog_candidate_tracked(&mut lifecycle, tx(90), &begin, &[], &end)
            .is_err()
    );
}

#[test]
fn catalog_action_pair_refuses_bad_echo_and_saturation_atomically() {
    let (mut connection, mut peer) = connect(dock());
    let activation = CatalogActivation {
        action: action(),
        catalog_generation: 9,
    };
    let acknowledged = ack(&activation.action);
    for field in 0..12 {
        let mut wrong = acknowledged.clone();
        match field {
            0 => wrong.grant.content_grant_epoch += 1,
            1 => wrong.output.generation += 1,
            2 => wrong.candidate_generation += 1,
            3 => wrong.presentation_epoch += 1,
            4 => wrong.interaction_generation += 1,
            5 => wrong.allocation.generation += 1,
            6 => wrong.target_id += 1,
            7 => wrong.target_generation += 1,
            8 => wrong.action_id += 1,
            9 => wrong.event_id += 1,
            10 => wrong.disposition = 2,
            11 => wrong.grant.connection_epoch += 1,
            _ => unreachable!(),
        }
        assert_eq!(
            connection.enqueue_catalog_action_response_tracked(
                tx(90),
                &wrong,
                Some((tx(91), &activation))
            ),
            Err(ShellClientError::WrongDirection),
            "changed field {field}"
        );
    }
    let mut pending = Vec::new();
    for id in 1..=63 {
        pending.push(
            connection
                .enqueue_catalog_action_response_tracked(tx(id), &ack(&action()), None)
                .unwrap()
                .first,
        );
    }
    assert_eq!(pending[0], Ticket(1));
    assert_eq!(
        connection.enqueue_catalog_action_response_tracked(
            tx(90),
            &ack(&action()),
            Some((tx(91), &activation))
        ),
        Err(ShellClientError::QueueSaturated)
    );
    for ticket in pending {
        commit(&mut connection, &mut peer, ticket);
    }
    let pair = connection
        .enqueue_catalog_action_response_tracked(
            tx(90),
            &ack(&action()),
            Some((tx(91), &activation)),
        )
        .unwrap();
    assert_eq!(pair.count, 2);
    let tickets: Vec<_> = pair.tickets().collect();
    let first = commit(&mut connection, &mut peer, tickets[0]);
    let second = commit(&mut connection, &mut peer, tickets[1]);
    assert_eq!(kind_and_id(&first).0, ShellFileKind::ActionAck);
    assert_eq!(
        decode_shell_file_catalog_action(&second, ShellFileKind::CatalogActivate)
            .unwrap()
            .record,
        ShellCatalogActionRecord::Activate(activation)
    );
}

#[test]
fn indicator_response_requires_the_same_consumed_action_before_queue_admission() {
    let (mut connection, mut peer) = connect(bar(
        SOPHIA_SHELL_CAPABILITY_VIEW_INDICATORS | SOPHIA_SHELL_CAPABILITY_INDICATOR_ACTIVATION
    ));
    let original = action();
    let activation = ShellIndicatorActivation {
        connection_epoch: EPOCH,
        snapshot_generation: 4,
        output: OutputId::from_raw(original.output.id),
        indicator: original.target_id,
        action: original.action_id,
        event_id: original.event_id,
    };
    for field in 0..6 {
        let mut wrong = ack(&original);
        match field {
            0 => wrong.disposition = 2,
            1 => wrong.event_id += 1,
            2 => wrong.grant.connection_epoch += 1,
            3 => wrong.output.id += 1,
            4 => wrong.target_id += 1,
            5 => wrong.action_id += 1,
            _ => unreachable!(),
        }
        assert_eq!(
            connection.enqueue_indicator_action_response_tracked(
                tx(90),
                &wrong,
                Some((tx(91), &activation))
            ),
            Err(ShellClientError::WrongDirection)
        );
    }
    let pair = connection
        .enqueue_indicator_action_response_tracked(
            tx(90),
            &ack(&original),
            Some((tx(91), &activation)),
        )
        .unwrap();
    assert_eq!(pair.first, Ticket(1));
    assert_eq!(pair.count, 2);
    let tickets: Vec<_> = pair.tickets().collect();
    let first = commit(&mut connection, &mut peer, tickets[0]);
    let second = commit(&mut connection, &mut peer, tickets[1]);
    assert_eq!(kind_and_id(&first).0, ShellFileKind::ActionAck);
    assert_eq!(
        decode_shell_file_indicator_activate(&second)
            .unwrap()
            .activation,
        activation
    );
}
