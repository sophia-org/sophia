//! Production file export and stores, with supplied protection and renderer completion.
use sophia_protocol::*;
use sophia_runtime::*;
#[allow(dead_code)]
#[path = "support/native_files_peer.rs"]
mod files;
#[path = "support/shell_file_peer.rs"]
mod shell_file_peer;
use files::*;

#[test]
fn native_files_assemble_exact_catalog_candidate_and_keeps_source_until_retirement() {
    let mut r = empty();
    let mut peer = Peer::connected(&mut r);
    let allocations = peer.allocation(&mut r);
    peer.upload(&mut r);
    peer.permit(&mut r);
    peer.candidate(&mut r);
    let c = catalog();
    assert_eq!(
        peer.transport
            .connection(&mut r)
            .service_native_launcher_content(context(&allocations), native(&c), 0)
            .unwrap(),
        3
    );
    let bundle = peer
        .transport
        .connection(&mut r)
        .begin_native_launcher_submission(1, context(&allocations), native(&c), 0)
        .unwrap();
    assert_eq!(bundle.native_launcher.unwrap().rows(), &[2, 1]);
    assert_eq!(
        bundle.resource(RESOURCE).unwrap().bytes(),
        &[0, 0, 255, 255, 0, 128, 0, 128]
    );
    peer.transport
        .content_prepared(&mut r, GRANT, OUTPUT, 1, 1, 1, 0)
        .unwrap();
    peer.transport
        .content_presented(&mut r, GRANT, OUTPUT, 1, 9, 1, 1)
        .unwrap();
    peer.transport.poll_io(&mut r).unwrap();
    for expected in [1, 2] {
        let (transaction, ShellContentRecord::CandidateOutcome(outcome)) =
            peer.read_content(&mut r)
        else {
            panic!()
        };
        assert_eq!(transaction, tx(20));
        assert_eq!(outcome.kind, expected);
        assert_eq!(outcome.candidate_generation, 1);
    }
    peer.transport.disconnect(&mut r).unwrap();
    r.collect();
    assert_eq!(r.accounting().memory.resident, 8);
    assert_eq!(r.accounting().retired_epochs, 1);
    drop(bundle);
    r.collect();
    assert_eq!(r.accounting().retired_epochs, 0);
}

#[test]
fn role_reservation_cannot_be_selected_or_widened_by_peer_capabilities() {
    for mode in 0..4 {
        let mut r = empty();
        let mut peer = Peer::with_limits(
            &mut r,
            if mode == 0 {
                ContentStoreProfile::Legacy
            } else {
                ContentStoreProfile::NativeLauncher
            },
            limits(),
        );
        let mut request = ShellV1ClientHello {
            minimum_revision: 7,
            maximum_revision: 7,
            required_capabilities: CAPS,
        };
        if mode == 1 {
            request.required_capabilities |= SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER;
        }
        if mode == 2 {
            request.maximum_revision = 6;
            request.minimum_revision = 5;
        }
        if mode == 3 {
            request.required_capabilities &= !SOPHIA_SHELL_CAPABILITY_CONTENT_DISCRETE_INPUT;
        }
        peer.refuse_offer(
            &mut r,
            request,
            ShellContentAdmissionPolicy::Granted {
                discrete_input: true,
            },
            if mode == 0 || mode == 2 {
                ShellTransportError::UnsupportedRevision
            } else {
                ShellTransportError::MissingCapability
            },
        );
        assert!(!peer.transport.supports_native_launcher());
        assert!(peer.transport.content_limits().is_none());
        assert_eq!(r.reserved_bytes(), 0);
    }
}

#[test]
fn operator_denial_is_an_encoded_refusal_and_retires_exact_reservation() {
    for (policy, reason) in [
        (ShellContentAdmissionPolicy::Unavailable, 4),
        (ShellContentAdmissionPolicy::Denied, 1),
        (
            ShellContentAdmissionPolicy::Granted {
                discrete_input: false,
            },
            1,
        ),
    ] {
        let mut r = empty();
        let mut peer = Peer::with_limits(&mut r, ContentStoreProfile::NativeLauncher, limits());
        peer.refuse_offer(
            &mut r,
            ShellV1ClientHello {
                minimum_revision: 7,
                maximum_revision: 7,
                required_capabilities: CAPS,
            },
            policy,
            ShellTransportError::ContentAdmissionRefused(ContentAdmissionRefused {
                reason,
                denied_capabilities: CAPS,
            }),
        );
        assert_eq!(r.reserved_bytes(), 0);
    }
}

#[test]
fn stale_catalog_at_end_rejects_one_owned_candidate_over_real_fifo() {
    let mut r = empty();
    let mut bounded = limits();
    bounded.max_frames_per_service_tick = 2;
    let mut peer = Peer::connected_with_limits(&mut r, bounded);
    let allocations = peer.allocation(&mut r);
    peer.upload(&mut r);
    peer.permit(&mut r);
    // The complete file transaction is in custody; pause its owner before End.
    peer.candidate(&mut r);
    let mut c = catalog();
    assert_eq!(
        peer.transport
            .service_native_launcher_content(&mut r, context(&allocations), native(&c), 0)
            .unwrap(),
        2
    );
    c.generation += 1;
    assert_eq!(
        peer.transport
            .service_native_launcher_content(&mut r, context(&allocations), native(&c), 0)
            .unwrap(),
        1
    );
    peer.transport.poll_io(&mut r).unwrap();
    let (transaction, ShellContentRecord::CandidateOutcome(v)) = peer.read_content(&mut r) else {
        panic!()
    };
    assert_eq!(transaction, tx(20));
    assert_eq!(v.kind, 3);
    assert_eq!(v.reason, ContentReason::Stale as u16);
    assert!(peer.transport.next_content_submission(&r).is_none());
}

#[test]
fn wrong_grant_and_legacy_allocation_refuse_before_ownership_change() {
    for wrong_grant in [true, false] {
        let mut r = empty();
        let mut peer = Peer::connected(&mut r);
        let before = r.accounting();
        if wrong_grant {
            let mut request = request(1);
            request.grant.connection_epoch += 1;
            peer.send(
                &mut r,
                ShellNativeLauncherRecord::AllocationRequest(request),
            );
        } else {
            let mut req = request(1);
            req.opening = 7;
            let legacy = ContentAllocationRequest {
                grant: GRANT,
                output: OUTPUT,
                allocation_request_id: 1,
                operation: 1,
                role: 1,
                edge: 1,
                prior: ContentAllocationId::default(),
                parent: ContentAllocationId::default(),
                parent_presentation_epoch: 0,
                anchor_parent_rect: ContentPixelRect::default(),
                desired_width: req.desired_width,
                desired_height: req.desired_height,
                margins: req.margins,
            };
            peer.send_content(&mut r, ShellContentRecord::AllocationRequest(legacy));
        }
        let c = catalog();
        for _ in 0..2 {
            assert_eq!(
                peer.transport
                    .service_native_launcher_content(&mut r, context(&[]), native(&c), 0),
                Err(if wrong_grant {
                    ShellTransportError::WrongContentGrant
                } else {
                    ShellTransportError::WrongContentRecord
                })
            );
        }
        assert!(peer.transport.next_content_allocation_request(&r).is_none());
        assert_eq!(r.accounting(), before);
    }
}

#[test]
fn native_visit_shares_record_budget_across_resources_and_demands() {
    let mut r = empty();
    let mut peer = Peer::connected(&mut r);
    let allocations = peer.allocation(&mut r);
    for demand_id in 1..=17 {
        // First record is resource traffic, the others are coalesced pacing.
        if demand_id == 1 {
            peer.send_content(
                &mut r,
                ShellContentRecord::ResourceRetire(ContentResourceRetire {
                    grant: GRANT,
                    resource: RESOURCE,
                }),
            );
        } else {
            peer.send_content(
                &mut r,
                ShellContentRecord::FrameDemand(ContentFrameDemand {
                    grant: GRANT,
                    output: OUTPUT,
                    allocation: ALLOCATION,
                    demand_id,
                    reason: 1,
                }),
            );
        }
    }
    let c = catalog();
    assert_eq!(
        peer.transport
            .service_native_launcher_content(&mut r, context(&allocations), native(&c), 0)
            .unwrap(),
        16
    );
    assert_eq!(
        peer.transport.next_content_demand(&r).unwrap().1.demand_id,
        16
    );
    assert_eq!(
        peer.transport
            .service_native_launcher_content(&mut r, context(&allocations), native(&c), 0)
            .unwrap(),
        1
    );
    assert_eq!(
        peer.transport.next_content_demand(&r).unwrap().1.demand_id,
        17
    );
}

#[test]
fn buffered_native_upload_resumes_exact_bounded_chunk_on_each_owner_visit() {
    let mut r = empty();
    let mut bounded = limits();
    bounded.max_frames_per_service_tick = 1;
    bounded.max_chunk_bytes = 32768;
    let mut peer = Peer::connected_with_limits(&mut r, bounded);
    let c = catalog();
    peer.begin_upload(
        &mut r,
        ContentResourceBegin {
            grant: GRANT,
            resource: RESOURCE,
            width_px: 8192,
            height_px: 2,
            rendered_scale_numerator: 1,
            rendered_scale_denominator: 1,
            pixel_format: 1,
            chunk_count: 2,
            total_bytes: 65536,
        },
    );
    assert_eq!(
        peer.transport
            .service_native_launcher_content(&mut r, context(&[]), native(&c), 0)
            .unwrap(),
        1
    );
    assert!(
        matches!(peer.read_content(&mut r).1, ShellContentRecord::ResourceStatus(v) if v.status == 1)
    );
    // Valid upload writes replace the socket's arbitrary ResourceChunk frames.
    // Both chunks are in custody before intake: I/O cannot hide a broken visit cap.
    let chunks = [vec![0; 32768], vec![128; 32768]];
    peer.upload_chunks(&mut r, &chunks);
    assert_eq!(
        peer.transport
            .service_native_launcher_content(&mut r, context(&[]), native(&c), 0)
            .unwrap(),
        1
    );
    assert_eq!(
        peer.transport
            .service_native_launcher_content(&mut r, context(&[]), native(&c), 0)
            .unwrap(),
        1
    );
    peer.no_event(&mut r);
    assert_eq!(
        peer.transport
            .service_native_launcher_content(&mut r, context(&[]), native(&c), 0)
            .unwrap(),
        0
    );
    peer.end_upload(
        &mut r,
        ContentResourceEnd {
            grant: GRANT,
            resource: RESOURCE,
            total_bytes: 65536,
            chunk_count: 2,
        },
    );
    assert_eq!(
        peer.transport
            .service_native_launcher_content(&mut r, context(&[]), native(&c), 0)
            .unwrap(),
        1
    );
    assert!(
        matches!(peer.read_content(&mut r).1, ShellContentRecord::ResourceStatus(v) if v.status == 2)
    );
    assert_eq!(
        peer.transport
            .lease_content_resource(&r, GRANT, RESOURCE)
            .unwrap()
            .bytes(),
        chunks.concat()
    );
}

#[test]
fn peer_eof_revokes_unconsumed_submissions_but_preserves_owned_requests() {
    for consumed in [false, true] {
        let mut r = empty();
        let mut peer = Peer::connected(&mut r);
        let c = catalog();
        peer.transport
            .publish_content_output_facts(&mut r, tx(1), 5, vec![facts()])
            .unwrap();
        peer.send(
            &mut r,
            ShellNativeLauncherRecord::AllocationRequest(request(1)),
        );
        if consumed {
            assert_eq!(
                peer.transport
                    .service_native_launcher_content(&mut r, context(&[]), native(&c), 0)
                    .unwrap(),
                1
            );
        }
        // Submitted is export custody, not allocation admission. On EOF the
        // export revokes untaken input; an already owned request survives.
        peer.shutdown();
        assert_eq!(
            peer.transport
                .service_native_launcher_content(&mut r, context(&[]), native(&c), 0),
            Err(ShellTransportError::NotConnected)
        );
        assert_eq!(
            peer.transport
                .next_content_allocation_request(&r)
                .map(|v| v.1.allocation_request_id),
            consumed.then_some(1)
        );
        assert_eq!(peer.transport.content_grant(), Some(GRANT));
        peer.transport.disconnect(&mut r).unwrap();
        r.collect();
        assert!(r.accounting().quiescent());
    }
}

#[test]
fn native_close_cancels_unsubmitted_work_but_retains_submitted_owner() {
    for phase in 0..4 {
        let mut r = empty();
        let mut bounded = limits();
        bounded.max_frames_per_service_tick = 1;
        let mut peer = Peer::connected_with_limits(&mut r, bounded);
        let allocations = peer.allocation(&mut r);
        peer.upload(&mut r);
        peer.permit(&mut r);
        let c = catalog();
        if phase > 0 {
            peer.candidate(&mut r);
            // Stop after Begin for phase 1; phases 2/3 own all three parts.
            for _ in 0..if phase == 1 { 1 } else { 3 } {
                assert_eq!(
                    peer.transport
                        .service_native_launcher_content(
                            &mut r,
                            context(&allocations),
                            native(&c),
                            0
                        )
                        .unwrap(),
                    1
                );
            }
        }
        let bundle = (phase == 3).then(|| {
            peer.transport
                .begin_native_launcher_submission(&mut r, 1, context(&allocations), native(&c), 0)
                .unwrap()
        });
        let before = r.accounting();
        let mut wrong = opening();
        wrong.opening += 1;
        assert!(
            peer.transport
                .close_native_launcher(&mut r, wrong, tx(80), ContentReason::Cancelled)
                .is_err()
        );
        assert_eq!(r.accounting(), before);
        peer.transport
            .close_native_launcher(&mut r, opening(), tx(80), ContentReason::Cancelled)
            .unwrap();
        peer.transport.poll_io(&mut r).unwrap();
        if phase == 0 {
            let (_, ShellContentRecord::FramePermit(v)) = peer.read_content(&mut r) else {
                panic!()
            };
            assert_eq!((v.state, v.reason), (3, ContentReason::Cancelled as u16));
        } else if phase != 3 {
            let (transaction, ShellContentRecord::CandidateOutcome(v)) = peer.read_content(&mut r)
            else {
                panic!()
            };
            assert_eq!(transaction, tx(20));
            assert_eq!(
                (v.kind, v.reason, v.candidate_generation),
                (3, ContentReason::Cancelled as u16, 1)
            );
        }
        assert!(matches!(
            peer.read_native(&mut r).1,
            ShellNativeLauncherRecord::Closed(_)
        ));
        assert_eq!(peer.transport.native_launcher_state(), None);
        assert_eq!(peer.transport.native_launcher_focus(), None);
        let store = r.active_candidates_mut(GRANT).unwrap();
        assert_eq!(store.pending_candidate_count(), 0);
        assert_eq!(store.submitted_candidate_count(), usize::from(phase == 3));
        if phase == 3 {
            assert_eq!(
                bundle
                    .as_ref()
                    .unwrap()
                    .resource(RESOURCE)
                    .unwrap()
                    .bytes()
                    .len(),
                8
            );
            peer.transport
                .content_prepared(&mut r, GRANT, OUTPUT, 1, 1, 1, 0)
                .unwrap();
            peer.transport
                .content_presented(&mut r, GRANT, OUTPUT, 1, 9, 1, 1)
                .unwrap();
            peer.transport.poll_io(&mut r).unwrap();
            for kind in [1, 2] {
                let (_, ShellContentRecord::CandidateOutcome(v)) = peer.read_content(&mut r) else {
                    panic!()
                };
                assert_eq!((v.kind, v.candidate_generation), (kind, 1));
            }
            assert_eq!(peer.transport.native_launcher_focus(), None);
        }
        // Extra service cannot duplicate terminal records.
        peer.transport.poll_io(&mut r).unwrap();
        peer.no_event(&mut r);
        // Active allocation remains owned: close is not pixel disappearance.
        assert_eq!(r.allocations_mut(GRANT).unwrap().snapshots(), allocations);
        peer.transport.disconnect(&mut r).unwrap();
        r.collect();
        if phase == 3 {
            assert_eq!(r.accounting().memory.resident, 8);
        }
        drop(bundle);
        r.collect();
        assert_eq!(r.accounting().retired_epochs, 0);
    }
}

#[test]
fn native_close_refuses_pending_allocation_and_cancels_standing_demand() {
    for proposal in [true, false] {
        let mut r = empty();
        let mut peer = Peer::connected(&mut r);
        let c = catalog();
        let allocations = if proposal {
            peer.transport
                .publish_content_output_facts(&mut r, tx(1), 5, vec![facts()])
                .unwrap();
            peer.transport.poll_io(&mut r).unwrap();
            assert!(matches!(
                peer.read_content(&mut r).1,
                ShellContentRecord::OutputFacts(_)
            ));
            peer.send(
                &mut r,
                ShellNativeLauncherRecord::AllocationRequest(request(1)),
            );
            vec![]
        } else {
            let allocations = peer.allocation(&mut r);
            peer.send_content(
                &mut r,
                ShellContentRecord::FrameDemand(ContentFrameDemand {
                    grant: GRANT,
                    output: OUTPUT,
                    allocation: ALLOCATION,
                    demand_id: 1,
                    reason: 1,
                }),
            );
            allocations
        };
        peer.transport
            .service_native_launcher_content(&mut r, context(&allocations), native(&c), 0)
            .unwrap();
        peer.transport
            .close_native_launcher(&mut r, opening(), tx(80), ContentReason::Cancelled)
            .unwrap();
        peer.transport.poll_io(&mut r).unwrap();
        let (transaction, record) = peer.read_content(&mut r);
        assert_eq!(transaction, tx(20));
        if proposal {
            let ShellContentRecord::AllocationResult(v) = record else {
                panic!()
            };
            assert_eq!(
                (v.status, v.reason, v.allocation_request_id),
                (2, ContentReason::Stale as u16, 1)
            );
            assert!(
                r.allocations_mut(GRANT)
                    .unwrap()
                    .pending_request()
                    .is_none()
            );
        } else {
            let ShellContentRecord::FramePermit(v) = record else {
                panic!()
            };
            assert_eq!(
                (v.state, v.reason, v.permit_id),
                (3, ContentReason::Cancelled as u16, 0)
            );
            assert!(
                r.active_candidates_mut(GRANT)
                    .unwrap()
                    .next_demand()
                    .is_none()
            );
        }
        assert!(matches!(
            peer.read_native(&mut r).1,
            ShellNativeLauncherRecord::Closed(_)
        ));
        peer.transport.disconnect(&mut r).unwrap();
        r.collect();
        assert_eq!(r.accounting().retired_epochs, 0);
    }
}

#[test]
fn late_closed_begin_gets_one_terminal_and_tails_cannot_reenter_submission() {
    let mut r = empty();
    let mut peer = Peer::connected(&mut r);
    let allocations = peer.allocation(&mut r);
    peer.upload(&mut r);
    peer.permit(&mut r);
    // These bytes are already in flight, but have not reached candidate intake.
    peer.candidate(&mut r);
    peer.transport
        .close_native_launcher(&mut r, opening(), tx(80), ContentReason::Cancelled)
        .unwrap();
    peer.transport.poll_io(&mut r).unwrap();
    assert!(matches!(
        peer.read_content(&mut r).1,
        ShellContentRecord::FramePermit(_)
    ));
    assert!(matches!(
        peer.read_native(&mut r).1,
        ShellNativeLauncherRecord::Closed(_)
    ));
    let mut wrong = opening();
    wrong.opening += 1;
    assert!(
        peer.transport
            .service_closed_native_content(&mut r, wrong, 0)
            .is_err()
    );
    assert_eq!(
        peer.transport
            .service_closed_native_content(&mut r, opening(), 0)
            .unwrap(),
        3
    );
    peer.transport.poll_io(&mut r).unwrap();
    let (transaction, ShellContentRecord::CandidateOutcome(v)) = peer.read_content(&mut r) else {
        panic!()
    };
    assert_eq!(transaction, tx(20));
    assert_eq!(
        (v.kind, v.reason, v.candidate_generation),
        (3, ContentReason::Cancelled as u16, 1)
    );
    let store = r.active_candidates_mut(GRANT).unwrap();
    assert_eq!(store.pending_candidate_count(), 0);
    assert_eq!(store.submitted_candidate_count(), 0);
    assert_eq!(r.allocations_mut(GRANT).unwrap().snapshots(), allocations);
    assert_eq!(peer.transport.native_launcher_focus(), None);
    // Duplicates of the terminalized generation do not create another outcome.
    peer.candidate(&mut r);
    assert_eq!(
        peer.transport
            .service_closed_native_content(&mut r, opening(), 0)
            .unwrap(),
        3
    );
    peer.transport.poll_io(&mut r).unwrap();
    peer.no_event(&mut r);
    peer.send(
        &mut r,
        ShellNativeLauncherRecord::AllocationRequest(request(2)),
    );
    peer.send_content(
        &mut r,
        ShellContentRecord::FrameDemand(ContentFrameDemand {
            grant: GRANT,
            output: OUTPUT,
            allocation: ALLOCATION,
            demand_id: 2,
            reason: 1,
        }),
    );
    assert_eq!(
        peer.transport
            .service_closed_native_content(&mut r, opening(), 0)
            .unwrap(),
        2
    );
    peer.transport.poll_io(&mut r).unwrap();
    // File intake selects pacing before the native allocation family.
    let (_, ShellContentRecord::FramePermit(v)) = peer.read_content(&mut r) else {
        panic!()
    };
    assert_eq!((v.state, v.demand_id, v.permit_id), (3, 2, 0));
    let (_, ShellContentRecord::AllocationResult(v)) = peer.read_content(&mut r) else {
        panic!()
    };
    assert_eq!((v.status, v.allocation_request_id), (2, 2));
    assert_eq!(r.allocations_mut(GRANT).unwrap().snapshots(), allocations);
    assert!(
        !peer
            .transport
            .closed_native_owners_settled(&r, opening())
            .unwrap()
    );
    for allocation in &allocations {
        peer.transport
            .invalidate_content_allocation(
                &mut r,
                TransactionId::from_raw(990),
                allocation.allocation,
                ContentReason::Revoked,
            )
            .unwrap();
    }
    peer.transport.poll_io(&mut r).unwrap();
    for _ in &allocations {
        assert!(matches!(peer.read_content(&mut r).1,
            ShellContentRecord::AllocationResult(v) if v.status == 4));
    }
    let held = peer
        .transport
        .lease_content_resource(&r, GRANT, RESOURCE)
        .unwrap();
    peer.send_content(
        &mut r,
        ShellContentRecord::ResourceRetire(ContentResourceRetire {
            grant: GRANT,
            resource: RESOURCE,
        }),
    );
    assert_eq!(
        peer.transport
            .service_closed_native_content(&mut r, opening(), 0)
            .unwrap(),
        1
    );
    peer.transport.poll_io(&mut r).unwrap();
    assert_eq!(r.accounting().memory.retiring, 8);
    assert!(
        !peer
            .transport
            .closed_native_owners_settled(&r, opening())
            .unwrap()
    );
    peer.no_event(&mut r);
    drop(held);
    assert_eq!(
        peer.transport
            .service_closed_native_content(&mut r, opening(), 0)
            .unwrap(),
        0
    );
    peer.transport.poll_io(&mut r).unwrap();
    assert!(matches!(
        peer.read_content(&mut r).1,
        ShellContentRecord::ResourceReleased(_)
    ));
    assert_eq!(r.accounting().memory.retiring, 0);
    assert!(
        peer.transport
            .closed_native_owners_settled(&r, opening())
            .unwrap()
    );
    peer.transport.disconnect(&mut r).unwrap();
    r.collect();
    assert_eq!(r.accounting().retired_epochs, 0);
}

#[test]
fn reopened_native_fifo_refuses_old_begin_without_consuming_new_permit() {
    let mut r = empty();
    let mut peer = Peer::connected(&mut r);
    peer.allocation(&mut r);
    peer.upload(&mut r);
    peer.permit(&mut r);
    peer.transport
        .close_native_launcher(&mut r, opening(), tx(800), ContentReason::Cancelled)
        .unwrap();
    peer.transport
        .invalidate_content_allocation(&mut r, tx(801), ALLOCATION, ContentReason::Revoked)
        .unwrap();
    peer.transport.poll_io(&mut r).unwrap();
    assert!(
        matches!(peer.read_content(&mut r).1, ShellContentRecord::FramePermit(v) if v.state == 3)
    );
    assert!(matches!(
        peer.read_native(&mut r).1,
        ShellNativeLauncherRecord::Closed(_)
    ));
    assert!(
        matches!(peer.read_content(&mut r).1, ShellContentRecord::AllocationResult(v) if v.status == 4)
    );
    let mut next = opening();
    next.opening += 1;
    peer.transport
        .publish_native_launcher_opening(&r, tx(802), next)
        .unwrap();
    peer.transport.poll_io(&mut r).unwrap();
    assert_eq!(
        peer.read_native(&mut r).1,
        ShellNativeLauncherRecord::Opening(next)
    );
    let c = catalog();
    let current = NativeLauncherCandidateContext {
        opening: next,
        state_revision: 1,
        catalog: &c,
    };
    let mut allocation_request = request(2);
    allocation_request.opening = next.opening;
    peer.send(
        &mut r,
        ShellNativeLauncherRecord::AllocationRequest(allocation_request),
    );
    peer.transport
        .service_native_launcher_content(&mut r, context(&[]), current, 0)
        .unwrap();
    let mut new_allocation = allocation();
    new_allocation.allocation.id = 2;
    new_allocation.native_opening = Some(next.opening);
    peer.transport
        .grant_content_allocation(&mut r, 2, new_allocation.clone(), &[])
        .unwrap();
    peer.transport.poll_io(&mut r).unwrap();
    assert!(
        matches!(peer.read_content(&mut r).1, ShellContentRecord::AllocationResult(v) if v.status == 1 && v.allocation_request_id == 2)
    );
    let allocations = [new_allocation];
    peer.send_content(
        &mut r,
        ShellContentRecord::FrameDemand(ContentFrameDemand {
            grant: GRANT,
            output: OUTPUT,
            allocation: allocations[0].allocation,
            demand_id: 2,
            reason: 1,
        }),
    );
    peer.transport
        .service_native_launcher_content(&mut r, context(&allocations), current, 0)
        .unwrap();
    peer.transport
        .grant_content_demand(&mut r, tx(803), OUTPUT, 2, 0)
        .unwrap();
    peer.transport.poll_io(&mut r).unwrap();
    assert!(
        matches!(peer.read_content(&mut r).1, ShellContentRecord::FramePermit(v) if v.state == 1 && v.demand_id == 2)
    );
    // Delayed old Begin and tails arrive after the successor owns a permit.
    peer.send(
        &mut r,
        ShellNativeLauncherRecord::AllocationRequest(request(3)),
    );
    peer.send_content(
        &mut r,
        ShellContentRecord::FrameDemand(ContentFrameDemand {
            grant: GRANT,
            output: OUTPUT,
            allocation: ALLOCATION,
            demand_id: 3,
            reason: 1,
        }),
    );
    peer.candidate(&mut r);
    peer.send_content(
        &mut r,
        ShellContentRecord::FrameDemandCancel(ContentFrameDemandCancel {
            grant: GRANT,
            output: OUTPUT,
            demand_id: 1,
            permit_id: 1,
        }),
    );
    assert_eq!(
        peer.transport
            .service_native_launcher_content(&mut r, context(&allocations), current, 0)
            .unwrap(),
        6
    );
    peer.transport.poll_io(&mut r).unwrap();
    // File intake drains a whole candidate's parts, then pacing, then allocation.
    assert!(matches!(peer.read_content(&mut r).1,
        ShellContentRecord::CandidateOutcome(v) if v.candidate_generation == 1 && v.kind == 3));
    assert!(matches!(peer.read_content(&mut r).1,
        ShellContentRecord::FramePermit(v) if v.demand_id == 3 && v.permit_id == 0 && v.state == 3));
    assert!(matches!(peer.read_content(&mut r).1,
        ShellContentRecord::AllocationResult(v) if v.allocation_request_id == 3 && v.status == 2));
    assert_eq!(r.accounting().permits, 1);
    let mut b = begin();
    b.opening = next.opening;
    b.content.candidate_generation = 2;
    b.content.pacing_permit = 2;
    let mut chunk = chunk();
    chunk.candidate_generation = 2;
    chunk.surfaces[0].allocation = allocations[0].allocation;
    peer.candidate_parts(&mut r, b, chunk);
    assert_eq!(
        peer.transport
            .service_native_launcher_content(&mut r, context(&allocations), current, 0)
            .unwrap(),
        3
    );
    let bundle = peer
        .transport
        .begin_native_launcher_submission(&mut r, 2, context(&allocations), current, 0)
        .unwrap();
    assert_eq!(bundle.native_launcher.unwrap().opening, next.opening);
    assert_eq!(bundle.candidate_generation, 2);
    peer.transport
        .content_prepared(&mut r, GRANT, OUTPUT, 2, 1, 1, 0)
        .unwrap();
    peer.transport
        .content_presented(&mut r, GRANT, OUTPUT, 2, 10, 1, 1)
        .unwrap();
    peer.transport.poll_io(&mut r).unwrap();
    for kind in [1, 2] {
        assert!(matches!(peer.read_content(&mut r).1,
            ShellContentRecord::CandidateOutcome(v) if v.candidate_generation == 2 && v.kind == kind));
    }
    peer.transport.disconnect(&mut r).unwrap();
    drop(bundle);
    r.collect();
    assert_eq!(r.accounting().retired_epochs, 0);
}
