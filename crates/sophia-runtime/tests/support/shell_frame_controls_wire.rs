use super::*;
use crate::shell_transport::outbox::tests::files::Peer;
use sophia_protocol::ContentReason;

fn key(stream_id: u64) -> ShellFrameStreamKey {
    let key = fixture::key(stream_id);
    ShellFrameStreamKey {
        grant: key.grant,
        output: key.output,
        allocation: key.allocation,
        facts_generation: key.facts_generation,
        stream_id,
    }
}

fn request(peer: &mut Peer, t: &mut Transport, record: ShellFrameRecord) {
    peer.submit(&mut t.transport.state, record.kind(), |header| {
        encode_shell_frame_record(header, &record, 13).unwrap()
    });
}

fn service(
    t: &mut Transport,
    f: &fixture::Fixture,
    present: bool,
    now: u64,
) -> Result<usize, ShellTransportError> {
    let contexts = [crate::ContentCandidateContext {
        output: fixture::output(),
        facts_generation: 6,
        interaction_generation: 8,
        allocations: &f.allocations,
    }];
    t.transport.state.service_content_frame_controls(
        &mut t.transport.content_epochs,
        if present { &contexts } else { &[] },
        now,
    )
}

fn expect(peer: &mut Peer, t: &mut Transport, record: ShellFrameRecord) {
    assert!(Peer::drain(&mut t.transport.state));
    peer.expect(&mut t.transport.state, &OutboundRecord::Frame(record));
}

fn refusal(
    transaction: u64,
    reason: ContentReason,
    request_kind: ShellFileKind,
) -> ShellFrameRecord {
    ShellFrameRecord::Refused {
        transaction: fixture::tx(transaction),
        grant: fixture::grant(),
        output: fixture::output(),
        stream_id: 1,
        candidate_generation: 0,
        request_kind,
        reason: reason as u16,
    }
}

fn opened(transaction: u64) -> ShellFrameRecord {
    ShellFrameRecord::Opened {
        transaction: fixture::tx(transaction),
        key: key(1),
        ttl_millis: ContentLimits::prototype(fixture::grant()).permit_timeout_ms,
    }
}

#[test]
fn wire_controls_reserve_before_take_and_budget_refusal_does_not_spend_the_stream() {
    let mut f = fixture::Fixture::new(64);
    let mut t = Transport::take(&mut f);
    let mut peer = Peer::attach_frame(&mut t.transport.state);
    let open = ShellFrameRecord::Open {
        transaction: fixture::tx(60),
        key: key(1),
    };
    request(&mut peer, &mut t, open.clone());
    let occupied = t
        .transport
        .content_epochs
        .control_occupancy(fixture::grant()) as u32;
    t.transport
        .state
        .content_limits
        .as_mut()
        .unwrap()
        .max_control_records = occupied;
    assert_eq!(service(&mut t, &f, true, 1), Ok(0));
    assert_eq!(
        t.transport
            .state
            .files_mut()
            .unwrap()
            .export()
            .peek_frame_request(),
        Some(&open)
    );
    t.transport
        .state
        .content_limits
        .as_mut()
        .unwrap()
        .max_control_records = occupied + 1;
    assert_eq!(service(&mut t, &f, true, 2), Ok(1));
    expect(
        &mut peer,
        &mut t,
        refusal(60, ContentReason::Budget, ShellFileKind::FrameStreamOpen),
    );
    assert!(
        t.transport
            .state
            .files_mut()
            .unwrap()
            .export()
            .peek_frame_request()
            .is_none()
    );
    request(
        &mut peer,
        &mut t,
        ShellFrameRecord::Open {
            transaction: fixture::tx(61),
            key: key(1),
        },
    );
    t.transport
        .state
        .content_limits
        .as_mut()
        .unwrap()
        .max_control_records = occupied + 2;
    assert_eq!(service(&mut t, &f, true, 3), Ok(1));
    expect(&mut peer, &mut t, opened(61));
    request(
        &mut peer,
        &mut t,
        ShellFrameRecord::Close {
            transaction: fixture::tx(62),
            key: key(1),
        },
    );
    t.transport
        .state
        .content_limits
        .as_mut()
        .unwrap()
        .max_control_records = occupied + 1;
    assert_eq!(
        service(&mut t, &f, true, 4),
        Ok(0),
        "Close reserves a refusal even with a lifetime close credit"
    );
    t.transport
        .state
        .content_limits
        .as_mut()
        .unwrap()
        .max_control_records = occupied + 2;
    assert_eq!(service(&mut t, &f, true, 5), Ok(1));
    expect(
        &mut peer,
        &mut t,
        ShellFrameRecord::Closed {
            transaction: fixture::tx(62),
            key: key(1),
            reason: ContentReason::Cancelled as u16,
        },
    );
    peer.quiet(&mut t.transport.state);
}

#[test]
fn missing_owner_context_is_a_typed_refusal_and_an_expired_close_is_nonfatal() {
    let mut f = fixture::Fixture::new(64);
    let mut t = Transport::take(&mut f);
    let mut peer = Peer::attach_frame(&mut t.transport.state);
    request(
        &mut peer,
        &mut t,
        ShellFrameRecord::Open {
            transaction: fixture::tx(70),
            key: key(1),
        },
    );
    assert_eq!(service(&mut t, &f, false, 1), Ok(1));
    expect(
        &mut peer,
        &mut t,
        refusal(70, ContentReason::Stale, ShellFileKind::FrameStreamOpen),
    );
    request(
        &mut peer,
        &mut t,
        ShellFrameRecord::Open {
            transaction: fixture::tx(71),
            key: key(1),
        },
    );
    assert_eq!(service(&mut t, &f, true, 2), Ok(1));
    expect(&mut peer, &mut t, opened(71));
    request(
        &mut peer,
        &mut t,
        ShellFrameRecord::Close {
            transaction: fixture::tx(72),
            key: key(1),
        },
    );
    let now = 3 + u64::from(ContentLimits::prototype(fixture::grant()).permit_timeout_ms);
    assert_eq!(service(&mut t, &f, false, now), Ok(1));
    expect(
        &mut peer,
        &mut t,
        ShellFrameRecord::Closed {
            transaction: fixture::tx(71),
            key: key(1),
            reason: ContentReason::Timeout as u16,
        },
    );
    expect(
        &mut peer,
        &mut t,
        refusal(72, ContentReason::Stale, ShellFileKind::FrameStreamClose),
    );
    peer.quiet(&mut t.transport.state);
}

#[test]
fn a_complete_frame_keeps_later_close_behind_it_in_the_file_queue() {
    let mut f = fixture::Fixture::new(64);
    let mut t = Transport::take(&mut f);
    let mut peer = Peer::attach_frame(&mut t.transport.state);
    request(
        &mut peer,
        &mut t,
        ShellFrameRecord::Open {
            transaction: fixture::tx(80),
            key: key(1),
        },
    );
    assert_eq!(service(&mut t, &f, true, 1), Ok(1));
    expect(&mut peer, &mut t, opened(80));
    let line = include_str!("../../../../protocol/golden/sophia-frame-transactions-v1.tsv")
        .lines()
        .find(|line| line.starts_with("commit-retained\t"))
        .unwrap();
    let hex = line.split('\t').nth(2).unwrap();
    let bytes: Vec<_> = hex
        .as_bytes()
        .chunks_exact(2)
        .map(|b| u8::from_str_radix(std::str::from_utf8(b).unwrap(), 16).unwrap())
        .collect();
    let ShellFrameRecord::Commit(mut commit) =
        decode_shell_frame_record(&bytes, ShellFileClass::Candidate, 13, 8192).unwrap()
    else {
        unreachable!();
    };
    commit.transaction = fixture::tx(81);
    commit.candidate.grant = fixture::grant();
    commit.candidate.output = fixture::output();
    commit.candidate.pacing_permit = 1;
    let frame = ShellFrameRecord::Commit(commit);
    request(&mut peer, &mut t, frame.clone());
    request(
        &mut peer,
        &mut t,
        ShellFrameRecord::Close {
            transaction: fixture::tx(82),
            key: key(1),
        },
    );
    assert_eq!(service(&mut t, &f, true, 2), Ok(0));
    assert_eq!(
        t.transport
            .state
            .files_mut()
            .unwrap()
            .export()
            .peek_frame_request(),
        Some(&frame)
    );
    assert_eq!(
        t.transport
            .state
            .files_mut()
            .unwrap()
            .export_mut()
            .take_frame_request(),
        Some(frame)
    );
    assert_eq!(service(&mut t, &f, true, 3), Ok(1));
    expect(
        &mut peer,
        &mut t,
        ShellFrameRecord::Closed {
            transaction: fixture::tx(82),
            key: key(1),
            reason: ContentReason::Cancelled as u16,
        },
    );
}

#[test]
fn a_foreign_content_grant_is_not_consumed_or_synthesized_into_a_reply() {
    let mut f = fixture::Fixture::new(64);
    let mut t = Transport::take(&mut f);
    let mut peer = Peer::attach_frame(&mut t.transport.state);
    let mut foreign = key(1);
    foreign.grant.content_grant_epoch += 1;
    let input = ShellFrameRecord::Open {
        transaction: fixture::tx(90),
        key: foreign,
    };
    request(&mut peer, &mut t, input.clone());
    assert_eq!(
        service(&mut t, &f, true, 1),
        Err(ShellTransportError::ContentCandidate(
            ContentCandidateError::Stale
        ))
    );
    assert_eq!(
        t.transport
            .state
            .files_mut()
            .unwrap()
            .export()
            .peek_frame_request(),
        Some(&input)
    );
    assert!(t.transport.state.output.front().is_none());
    assert!(t.candidates().pending_frame_stream_event().is_none());
}

#[test]
fn controls_obey_the_existing_per_service_tick_limit() {
    let mut f = fixture::Fixture::new(64);
    let mut t = Transport::take(&mut f);
    let mut peer = Peer::attach_frame(&mut t.transport.state);
    request(
        &mut peer,
        &mut t,
        ShellFrameRecord::Open {
            transaction: fixture::tx(100),
            key: key(1),
        },
    );
    let close = ShellFrameRecord::Close {
        transaction: fixture::tx(101),
        key: key(1),
    };
    request(&mut peer, &mut t, close.clone());
    t.transport
        .state
        .content_limits
        .as_mut()
        .unwrap()
        .max_frames_per_service_tick = 1;
    assert_eq!(service(&mut t, &f, true, 1), Ok(1));
    assert_eq!(
        t.transport
            .state
            .files_mut()
            .unwrap()
            .export()
            .peek_frame_request(),
        Some(&close)
    );
    expect(&mut peer, &mut t, opened(100));
    assert_eq!(service(&mut t, &f, true, 2), Ok(1));
    expect(
        &mut peer,
        &mut t,
        ShellFrameRecord::Closed {
            transaction: fixture::tx(101),
            key: key(1),
            reason: ContentReason::Cancelled as u16,
        },
    );
}

fn admitted_retained() -> (fixture::Fixture, Transport, Peer) {
    let mut f = fixture::Fixture::new(64);
    let mut t = Transport::take(&mut f);
    let mut peer = Peer::attach_frame(&mut t.transport.state);
    request(
        &mut peer,
        &mut t,
        ShellFrameRecord::Open {
            transaction: fixture::tx(100),
            key: key(1),
        },
    );
    assert_eq!(service(&mut t, &f, true, 1), Ok(1));
    expect(&mut peer, &mut t, opened(100));
    let allocation = &f.allocations[0];
    let scene = sophia_protocol::ContentCandidate {
        grant: fixture::grant(),
        candidate_generation: 1,
        output: fixture::output(),
        facts_generation: 6,
        pacing_permit: 1,
        interaction_generation: 8,
        surfaces: vec![sophia_protocol::ContentSurface {
            allocation: allocation.allocation,
            scale_generation: allocation.scale_generation,
            role: allocation.role,
            edge: allocation.edge,
            margins: allocation.margins,
            reservation_extent: allocation.allowed_reservation_extent,
            parent_surface_index: u16::MAX,
            anchor_parent_rect: allocation.anchor_parent_rect,
        }],
        placements: vec![sophia_protocol::ContentPlacement {
            resource: fixture::resource(2),
            surface_index: 0,
            destination_x_px: 0,
            destination_y_px: 0,
        }],
        targets: vec![],
    };
    request(
        &mut peer,
        &mut t,
        ShellFrameRecord::Commit(ShellFrameCommit {
            transaction: fixture::tx(101),
            candidate: scene,
            image: None,
            retire: None,
        }),
    );
    let mut owner = crate::shell_transport::owned_copy::OwnedCopyOwner::default();
    let mut worker = crate::IsbValidationWorker::new(1, 8).unwrap();
    owner
        .service(
            &mut t.transport.state,
            &mut t.transport.content_epochs,
            &mut worker,
            2,
            std::time::Instant::now(),
            0,
        )
        .unwrap();
    assert_eq!(owner.pending(), 0);
    assert_eq!(worker.pending(), 0);
    assert_eq!(
        t.candidates().ready_frame_commit(),
        Some((fixture::output(), 1))
    );
    assert!(
        t.transport
            .state
            .files_mut()
            .unwrap()
            .export()
            .peek_frame_request()
            .is_none()
    );
    assert!(
        t.transport.state.output.front().is_none(),
        "admission is not terminal or Prepared"
    );
    (f, t, peer)
}

#[test]
fn retained_complete_scene_uses_no_ring_or_worker_and_keeps_the_stream_credit() {
    let (f, mut t, mut peer) = admitted_retained();
    assert_eq!(finish_ready(&mut t, &f.allocations, 6, 8, true, 3), Ok(1));
    assert_eq!(t.candidates().pending_candidate_count(), 1);
    let _display = t
        .transport
        .begin_content_submission(fixture::output(), 1, 4)
        .unwrap();
    t.transport
        .content_prepared(fixture::grant(), fixture::output(), 1, 20, 30, 5)
        .unwrap();
    let OutboundRecord::Frame(prepared) = &t.transport.state.output.front().unwrap().record else {
        panic!()
    };
    assert!(
        matches!(prepared, ShellFrameRecord::Outcome { outcome, credit_expires_msec: 0, .. } if outcome.kind == 1)
    );
    let prepared = prepared.clone();
    expect(&mut peer, &mut t, prepared);
    t.transport
        .content_presented_at(fixture::grant(), fixture::output(), 1, 40, 20, 30, 50)
        .unwrap();
    let OutboundRecord::Frame(presented) = &t.transport.state.output.front().unwrap().record else {
        panic!()
    };
    assert!(
        matches!(presented, ShellFrameRecord::Outcome { outcome, credit_expires_msec, .. }
        if outcome.kind == 2 && *credit_expires_msec == 50 + u64::from(ContentLimits::prototype(fixture::grant()).permit_timeout_ms))
    );
    let presented = presented.clone();
    expect(&mut peer, &mut t, presented);
    peer.quiet(&mut t.transport.state);
}

fn finish_ready(
    t: &mut Transport,
    allocations: &[crate::ContentAllocationSnapshot],
    facts: u64,
    interaction: u64,
    present: bool,
    now: u64,
) -> Result<usize, ShellTransportError> {
    let contexts = [crate::ContentCandidateContext {
        output: fixture::output(),
        facts_generation: facts,
        interaction_generation: interaction,
        allocations,
    }];
    t.transport.state.service_content_candidates(
        &mut t.transport.content_epochs,
        if present { &contexts } else { &[] },
        now,
    )
}

#[test]
fn ready_scene_rechecks_facts_allocations_interaction_and_output_presence() {
    for fault in 0..4 {
        let (f, mut t, _peer) = admitted_retained();
        let allocations = if fault == 1 {
            &[][..]
        } else {
            &f.allocations[..]
        };
        assert_eq!(
            finish_ready(
                &mut t,
                allocations,
                if fault == 0 { 7 } else { 6 },
                if fault == 2 { 9 } else { 8 },
                fault != 3,
                3
            ),
            Ok(1)
        );
        assert_eq!(t.candidates().pending_candidate_count(), 0);
        assert!(t.candidates().ready_frame_commit().is_none());
        if fault != 2 {
            assert!(
                matches!(t.take_record(), ShellFrameRecord::Closed { reason, .. }
                if reason == ContentReason::AllocationLost as u16)
            );
        }
        let ShellFrameRecord::Outcome {
            outcome,
            credit_expires_msec,
            ..
        } = t.take_record()
        else {
            panic!()
        };
        assert_eq!(outcome.kind, 3);
        assert_eq!(
            outcome.reason,
            if fault == 3 {
                ContentReason::AllocationLost
            } else {
                ContentReason::Stale
            } as u16
        );
        assert_eq!(credit_expires_msec != 0, fault == 2);
        assert_eq!(
            finish_ready(&mut t, allocations, 6, 8, fault != 3, 4),
            Ok(0)
        );
        assert!(t.transport.state.output.front().is_none());
    }
}

#[test]
fn ready_scene_waits_for_output_capacity_and_shares_the_control_service_bound() {
    let (f, mut t, mut peer) = admitted_retained();
    t.transport
        .state
        .content_limits
        .as_mut()
        .unwrap()
        .max_control_records = 0;
    assert_eq!(finish_ready(&mut t, &f.allocations, 6, 8, true, 3), Ok(0));
    assert_eq!(
        t.candidates().ready_frame_commit(),
        Some((fixture::output(), 1))
    );
    t.transport
        .state
        .content_limits
        .as_mut()
        .unwrap()
        .max_control_records = 64;
    t.transport
        .state
        .content_limits
        .as_mut()
        .unwrap()
        .max_frames_per_service_tick = 1;
    request(
        &mut peer,
        &mut t,
        ShellFrameRecord::Close {
            transaction: fixture::tx(102),
            key: key(1),
        },
    );
    assert_eq!(finish_ready(&mut t, &f.allocations, 6, 8, true, 4), Ok(1));
    assert_eq!(
        t.candidates().ready_frame_commit(),
        Some((fixture::output(), 1)),
        "Close used this pass's service slot"
    );
    expect(
        &mut peer,
        &mut t,
        ShellFrameRecord::Closed {
            transaction: fixture::tx(102),
            key: key(1),
            reason: ContentReason::Cancelled as u16,
        },
    );
    assert_eq!(finish_ready(&mut t, &f.allocations, 6, 8, true, 5), Ok(1));
    assert_eq!(
        t.candidates().pending_candidate_count(),
        1,
        "Close does not discard previously admitted work"
    );
}
