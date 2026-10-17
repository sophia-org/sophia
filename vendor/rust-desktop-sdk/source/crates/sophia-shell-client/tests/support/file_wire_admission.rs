use super::*;

#[test]
fn capacity_reservation_survives_service_and_excludes_other_admissions() {
    let (mut connection, mut peer) = connect(bar(0));
    let token = connection.reserve_capacity(2, 4096).unwrap();
    let pending: Vec<_> = (1..=30)
        .map(|id| enqueue_demand(&mut connection, id))
        .collect();
    assert!(matches!(
        connection.reserve_capacity(1, 100),
        Err(ShellClientError::QueueSaturated)
    ));
    assert_eq!(
        connection.enqueue_content_tracked(tx(31), &demand(31)),
        Err(ShellClientError::QueueSaturated)
    );
    commit(&mut connection, &mut peer, pending[0]);
    let extra = enqueue_demand(&mut connection, 31);
    let admission = connection
        .commit_reserved_content(&token, tx(32), &demand(32))
        .unwrap();
    assert_eq!(admission.first, Ticket(32));
    assert_eq!(connection.custody(admission.first), Some(Custody::Queued));
    assert!(matches!(
        connection.cancel_reservation(&token),
        Err(ShellClientError::InvalidArgument(_))
    ));
    for ticket in pending.into_iter().skip(1).chain([extra, admission.first]) {
        commit(&mut connection, &mut peer, ticket);
    }
}

#[test]
fn capacity_token_is_connection_bound_and_invalid_commits_retain_it() {
    let (mut connection, mut peer) = connect(bar(0));
    let (mut other, _) = connect(bar(0));
    let token = connection.reserve_capacity(1, 4096).unwrap();
    let other_token = other.reserve_capacity(1, 4096).unwrap();
    assert!(matches!(
        other.cancel_reservation(&token),
        Err(ShellClientError::InvalidArgument(_))
    ));
    assert!(matches!(
        other.commit_reserved_content(&token, tx(1), &demand(1)),
        Err(ShellClientError::InvalidArgument(_))
    ));
    other.cancel_reservation(&other_token).unwrap();
    assert!(
        connection
            .commit_reserved_content_group(&token, tx(1), &[])
            .is_err()
    );
    assert_eq!(connection.custody(Ticket(1)), None);
    let admission = connection
        .commit_reserved_content_group(&token, tx(2), &withdrawal())
        .unwrap();
    assert_eq!(admission.first, Ticket(1));
    commit(&mut connection, &mut peer, admission.first);
    let replacement = connection.reserve_capacity(1, 4096).unwrap();
    assert!(matches!(
        connection.commit_reserved_content(&token, tx(3), &demand(3)),
        Err(ShellClientError::InvalidArgument(_))
    ));
    connection.cancel_reservation(&replacement).unwrap();
}

#[test]
fn capacity_reservation_size_refusal_cancel_and_terminal_release() {
    let (mut connection, mut peer) = connect(bar(0));
    assert!(matches!(
        connection.reserve_capacity(0, 1),
        Err(ShellClientError::InvalidArgument(_))
    ));
    let small = connection.reserve_capacity(1, 1).unwrap();
    assert!(matches!(
        connection.commit_reserved_content(&small, tx(1), &demand(1)),
        Err(ShellClientError::InvalidArgument(_))
    ));
    assert_eq!(connection.custody(Ticket(1)), None);
    connection.cancel_reservation(&small).unwrap();
    let token = connection.reserve_capacity(32, 4096).unwrap();
    peer.close();
    let _ = connection.poll_io();
    assert!(
        connection
            .commit_reserved_content(&token, tx(2), &demand(2))
            .is_err()
    );
    assert_eq!(connection.custody(Ticket(1)), None);
    assert!(matches!(
        connection.cancel_reservation(&token),
        Err(ShellClientError::InvalidArgument(_))
    ));
}

#[test]
fn content_reservation_cancel_and_commit_issue_only_one_ticket_without_io() {
    let (mut connection, mut peer) = connect(bar(0));
    for _ in 0..3 {
        let reservation = connection.reserve_content(tx(1), &demand(1)).unwrap();
        peer.pump();
        assert_eq!(peer.walk_count("transaction"), 0);
        reservation.cancel();
        assert_eq!(connection.custody(Ticket(1)), None);
    }
    drop(connection.reserve_content(tx(2), &demand(2)).unwrap());
    let reservation = connection.reserve_content(tx(3), &demand(3)).unwrap();
    let admission = reservation.commit();
    assert_eq!(
        admission,
        Admission {
            first: Ticket(1),
            count: 1
        }
    );
    assert_eq!(connection.custody(admission.first), Some(Custody::Queued));
    peer.pump();
    assert_eq!(peer.walk_count("transaction"), 0, "commit did socket I/O");
    let record = commit(&mut connection, &mut peer, admission.first);
    assert_eq!(kind_and_id(&record).0, ShellFileKind::FrameDemand);
}

#[test]
fn content_reservation_refuses_capacity_and_invalid_groups_without_tickets() {
    let (mut connection, mut peer) = connect(bar(0));
    assert!(connection.reserve_content_group(tx(1), &[]).is_err());
    let reservation = connection
        .reserve_content_group(tx(2), &withdrawal())
        .unwrap();
    reservation.cancel();
    let pending = fill_bulk(&mut connection);
    assert!(matches!(
        connection.reserve_content(tx(90), &demand(90)),
        Err(ShellClientError::QueueSaturated)
    ));
    assert_eq!(connection.custody(Ticket(33)), None);
    for ticket in pending {
        commit(&mut connection, &mut peer, ticket);
    }
    let admission = connection
        .reserve_content_group(tx(3), &withdrawal())
        .unwrap()
        .commit();
    assert_eq!(admission.first, Ticket(33));
    assert_eq!(admission.count, 1);
    let record = commit(&mut connection, &mut peer, admission.first);
    assert_eq!(
        decode_shell_file_candidate(&record).unwrap().transaction,
        tx(3)
    );
}

#[test]
fn cancelled_resource_reservations_do_not_consume_upload_slots() {
    let (mut connection, mut peer) = connect(Profile {
        limits_published: true,
        ..bar(0)
    });
    let (_, limits) = next(&mut connection, &mut peer, ShellConnection::take_content);
    assert!(matches!(limits, ShellContentRecord::Limits(_)));
    let begin = ShellContentRecord::ResourceBegin(ContentResourceBegin {
        grant: grant(),
        resource: ContentResourceId {
            id: 1,
            generation: 1,
        },
        width_px: 4,
        height_px: 4,
        rendered_scale_numerator: 1,
        rendered_scale_denominator: 1,
        pixel_format: 1,
        chunk_count: 1,
        total_bytes: 64,
    });
    for _ in 0..16 {
        drop(connection.reserve_content(tx(1), &begin).unwrap());
    }
    let admission = connection.reserve_content(tx(2), &begin).unwrap().commit();
    assert_eq!(admission.first, Ticket(1));
    let record = commit(&mut connection, &mut peer, admission.first);
    assert_eq!(kind_and_id(&record).0, ShellFileKind::ResourceBegin);
}

#[test]
fn reservation_does_not_promise_custody_after_peer_disconnect() {
    let (mut connection, mut peer) = connect(bar(0));
    let reservation = connection.reserve_content(tx(1), &demand(1)).unwrap();
    peer.close();
    let admission = reservation.commit();
    assert_eq!(connection.custody(admission.first), Some(Custody::Queued));
    let _ = connection.poll_io();
    assert_eq!(
        connection.custody(admission.first),
        Some(Custody::DroppedUnsent)
    );
    assert!(connection.reserve_content(tx(2), &demand(2)).is_err());
}

pub(super) fn withdrawal() -> [ShellContentRecord; 2] {
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
