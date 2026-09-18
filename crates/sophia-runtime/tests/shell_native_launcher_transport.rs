//! Actual socket/store controls with supplied protection and renderer completion.
use sophia_protocol::*;
use sophia_runtime::*;
use std::io::Read;
#[path = "support/native_launcher_socket.rs"]
mod socket;
use socket::*;

#[test]
fn native_socket_assembles_exact_catalog_candidate_and_keeps_source_until_retirement() {
    let mut r = empty();
    let mut peer = Peer::connected(&mut r);
    let allocations = peer.allocation(&mut r);
    peer.upload(&mut r);
    peer.permit(&mut r);
    peer.send(ShellNativeLauncherRecord::CandidateBegin(begin()));
    peer.send(ShellNativeLauncherRecord::CandidateChunk(chunk()));
    peer.send_content(ShellContentRecord::CandidateEnd(end()));
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
            decode_shell_content_frame(&peer.read()).unwrap()
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
        let mut peer = Peer::new(
            &mut r,
            if mode == 0 {
                ContentStoreProfile::Legacy
            } else {
                ContentStoreProfile::NativeLauncher
            },
        );
        let mut request = hello();
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
        assert!(peer.negotiate(&mut r, request, granted()).is_err());
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
        let mut peer = Peer::new(&mut r, ContentStoreProfile::NativeLauncher);
        assert!(
            matches!(peer.negotiate(&mut r,hello(),policy),Err(ShellTransportError::ContentAdmissionRefused(v)) if v.reason==reason)
        );
        assert!(
            matches!(decode_shell_content_frame(&peer.read()).unwrap().1,ShellContentRecord::AdmissionRefused(v) if v.reason==reason && v.denied_capabilities==CAPS)
        );
        assert_eq!(r.reserved_bytes(), 0);
    }
}

#[test]
fn stale_catalog_at_end_rejects_one_owned_candidate_over_real_fifo() {
    let mut r = empty();
    let mut peer = Peer::connected(&mut r);
    let allocations = peer.allocation(&mut r);
    peer.upload(&mut r);
    peer.permit(&mut r);
    peer.send(ShellNativeLauncherRecord::CandidateBegin(begin()));
    peer.send(ShellNativeLauncherRecord::CandidateChunk(chunk()));
    let mut c = catalog();
    assert_eq!(
        peer.transport
            .service_native_launcher_content(&mut r, context(&allocations), native(&c), 0)
            .unwrap(),
        2
    );
    c.generation += 1;
    peer.send_content(ShellContentRecord::CandidateEnd(end()));
    assert_eq!(
        peer.transport
            .service_native_launcher_content(&mut r, context(&allocations), native(&c), 0)
            .unwrap(),
        1
    );
    peer.transport.poll_io(&mut r).unwrap();
    let (transaction, ShellContentRecord::CandidateOutcome(v)) =
        decode_shell_content_frame(&peer.read()).unwrap()
    else {
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
        if wrong_grant {
            let mut request = request(1);
            request.grant.connection_epoch += 1;
            peer.send(ShellNativeLauncherRecord::AllocationRequest(request));
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
            peer.send_content(ShellContentRecord::AllocationRequest(legacy));
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
            peer.send_content(ShellContentRecord::ResourceRetire(ContentResourceRetire {
                grant: GRANT,
                resource: RESOURCE,
            }));
        } else {
            peer.send_content(ShellContentRecord::FrameDemand(ContentFrameDemand {
                grant: GRANT,
                output: OUTPUT,
                allocation: ALLOCATION,
                demand_id,
                reason: 1,
            }));
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
fn buffered_native_visit_stops_at_payload_budget_and_resumes_exact_record() {
    let mut r = empty();
    let mut peer = Peer::connected(&mut r);
    let c = catalog();
    for ordinal in 0..2 {
        peer.send_content(ShellContentRecord::ResourceChunk(ContentResourceChunk {
            grant: GRANT,
            resource: RESOURCE,
            ordinal,
            offset: u64::from(ordinal) * 32768,
            bytes: vec![0; 32768],
        }));
    }
    // Supply an already-buffered pair so I/O's own bound cannot mask a broken
    // dispatch byte budget. Unknown resources produce ordinary typed refusals.
    peer.transport.poll_io(&mut r).unwrap();
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
    peer.transport.poll_io(&mut r).unwrap();
    for _ in 0..2 {
        assert!(
            matches!(decode_shell_content_frame(&peer.read()).unwrap().1,ShellContentRecord::ResourceStatus(v) if v.status==3)
        );
    }
    assert_eq!(
        peer.transport
            .service_native_launcher_content(&mut r, context(&[]), native(&c), 0)
            .unwrap(),
        0
    );
}

#[test]
fn peer_eof_reports_disconnect_after_buffered_native_request_is_owned() {
    let mut r = empty();
    let mut peer = Peer::connected(&mut r);
    let c = catalog();
    peer.transport
        .publish_content_output_facts(&mut r, tx(1), 5, vec![facts()])
        .unwrap();
    peer.send(ShellNativeLauncherRecord::AllocationRequest(request(1)));
    peer.client.shutdown(std::net::Shutdown::Write).unwrap();
    assert_eq!(
        peer.transport
            .service_native_launcher_content(&mut r, context(&[]), native(&c), 0)
            .unwrap(),
        1
    );
    assert_eq!(
        peer.transport
            .next_content_allocation_request(&r)
            .unwrap()
            .1
            .allocation_request_id,
        1
    );
    assert_eq!(
        peer.transport
            .service_native_launcher_content(&mut r, context(&[]), native(&c), 0),
        Err(ShellTransportError::NotConnected)
    );
    assert!(peer.transport.next_content_allocation_request(&r).is_some());
    peer.transport.disconnect(&mut r).unwrap();
    r.collect();
    assert!(r.accounting().quiescent());
}

#[path = "support/native_launcher_focus.rs"]
mod focus;

#[test]
fn native_close_cancels_unsubmitted_work_but_retains_submitted_owner() {
    for phase in 0..4 {
        let mut r = empty();
        let mut peer = Peer::connected(&mut r);
        let allocations = peer.allocation(&mut r);
        peer.upload(&mut r);
        peer.permit(&mut r);
        let c = catalog();
        if phase > 0 {
            peer.send(ShellNativeLauncherRecord::CandidateBegin(begin()));
            if phase > 1 {
                peer.send(ShellNativeLauncherRecord::CandidateChunk(chunk()));
                peer.send_content(ShellContentRecord::CandidateEnd(end()));
            }
            peer.transport
                .service_native_launcher_content(&mut r, context(&allocations), native(&c), 0)
                .unwrap();
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
            let (_, ShellContentRecord::FramePermit(v)) =
                decode_shell_content_frame(&peer.read()).unwrap()
            else {
                panic!()
            };
            assert_eq!((v.state, v.reason), (3, ContentReason::Cancelled as u16));
        } else if phase != 3 {
            let (transaction, ShellContentRecord::CandidateOutcome(v)) =
                decode_shell_content_frame(&peer.read()).unwrap()
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
            decode_shell_native_launcher_frame(&peer.read()).unwrap().1,
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
                let (_, ShellContentRecord::CandidateOutcome(v)) =
                    decode_shell_content_frame(&peer.read()).unwrap()
                else {
                    panic!()
                };
                assert_eq!((v.kind, v.candidate_generation), (kind, 1));
            }
            assert_eq!(peer.transport.native_launcher_focus(), None);
        }
        // Extra service cannot duplicate terminal records.
        peer.transport.poll_io(&mut r).unwrap();
        peer.client.set_nonblocking(true).unwrap();
        assert_eq!(
            peer.client.read(&mut [0]).unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
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
            let _facts = peer.read();
            peer.send(ShellNativeLauncherRecord::AllocationRequest(request(1)));
            vec![]
        } else {
            let allocations = peer.allocation(&mut r);
            peer.send_content(ShellContentRecord::FrameDemand(ContentFrameDemand {
                grant: GRANT,
                output: OUTPUT,
                allocation: ALLOCATION,
                demand_id: 1,
                reason: 1,
            }));
            allocations
        };
        peer.transport
            .service_native_launcher_content(&mut r, context(&allocations), native(&c), 0)
            .unwrap();
        peer.transport
            .close_native_launcher(&mut r, opening(), tx(80), ContentReason::Cancelled)
            .unwrap();
        peer.transport.poll_io(&mut r).unwrap();
        let (transaction, record) = decode_shell_content_frame(&peer.read()).unwrap();
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
            decode_shell_native_launcher_frame(&peer.read()).unwrap().1,
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
    peer.send(ShellNativeLauncherRecord::CandidateBegin(begin()));
    peer.send(ShellNativeLauncherRecord::CandidateChunk(chunk()));
    peer.send_content(ShellContentRecord::CandidateEnd(end()));
    peer.transport
        .close_native_launcher(&mut r, opening(), tx(80), ContentReason::Cancelled)
        .unwrap();
    peer.transport.poll_io(&mut r).unwrap();
    assert!(matches!(
        decode_shell_content_frame(&peer.read()).unwrap().1,
        ShellContentRecord::FramePermit(_)
    ));
    assert!(matches!(
        decode_shell_native_launcher_frame(&peer.read()).unwrap().1,
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
    let (transaction, ShellContentRecord::CandidateOutcome(v)) =
        decode_shell_content_frame(&peer.read()).unwrap()
    else {
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
    peer.send(ShellNativeLauncherRecord::CandidateBegin(begin()));
    peer.send(ShellNativeLauncherRecord::CandidateChunk(chunk()));
    peer.send_content(ShellContentRecord::CandidateEnd(end()));
    assert_eq!(
        peer.transport
            .service_closed_native_content(&mut r, opening(), 0)
            .unwrap(),
        3
    );
    peer.transport.poll_io(&mut r).unwrap();
    peer.client.set_nonblocking(true).unwrap();
    assert_eq!(
        peer.client.read(&mut [0]).unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
    peer.client.set_nonblocking(false).unwrap();
    peer.send(ShellNativeLauncherRecord::AllocationRequest(request(2)));
    peer.send_content(ShellContentRecord::FrameDemand(ContentFrameDemand {
        grant: GRANT,
        output: OUTPUT,
        allocation: ALLOCATION,
        demand_id: 2,
        reason: 1,
    }));
    assert_eq!(
        peer.transport
            .service_closed_native_content(&mut r, opening(), 0)
            .unwrap(),
        2
    );
    peer.transport.poll_io(&mut r).unwrap();
    let (_, ShellContentRecord::AllocationResult(v)) =
        decode_shell_content_frame(&peer.read()).unwrap()
    else {
        panic!()
    };
    assert_eq!((v.status, v.allocation_request_id), (2, 2));
    let (_, ShellContentRecord::FramePermit(v)) = decode_shell_content_frame(&peer.read()).unwrap()
    else {
        panic!()
    };
    assert_eq!((v.state, v.demand_id, v.permit_id), (3, 2, 0));
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
        assert!(
            matches!(decode_shell_content_frame(&peer.read()).unwrap().1,
            ShellContentRecord::AllocationResult(v) if v.status == 4)
        );
    }
    let held = peer
        .transport
        .lease_content_resource(&r, GRANT, RESOURCE)
        .unwrap();
    peer.send_content(ShellContentRecord::ResourceRetire(ContentResourceRetire {
        grant: GRANT,
        resource: RESOURCE,
    }));
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
    peer.client.set_nonblocking(true).unwrap();
    assert_eq!(
        peer.client.read(&mut [0]).unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
    peer.client.set_nonblocking(false).unwrap();
    drop(held);
    assert_eq!(
        peer.transport
            .service_closed_native_content(&mut r, opening(), 0)
            .unwrap(),
        0
    );
    peer.transport.poll_io(&mut r).unwrap();
    assert!(matches!(
        decode_shell_content_frame(&peer.read()).unwrap().1,
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
