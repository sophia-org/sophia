use super::*;

#[test]
fn refused_begins_release_upload_slots_without_a_status_event() {
    let (mut c, mut p, mut l) = setup();
    for n in 1..=6 {
        let mut begin = begin();
        begin.resource.id = n;
        let id = begin.resource;
        let a = c.begin_native_resource(&mut l, tx(80 + n), begin).unwrap();
        refuse(&mut c, &mut p, a.first);
        l.service(&c).unwrap();
        assert_eq!(l.resource_state(id), None);
    }
}

#[test]
fn payload_queued_behind_a_refused_begin_does_not_stall_the_queue() {
    let (mut c, mut p, mut l) = setup();
    let a = c.begin_native_resource(&mut l, tx(80), begin()).unwrap();
    let payload = c
        .enqueue_content_tracked(
            tx(81),
            &ShellContentRecord::ResourceChunk(ContentResourceChunk {
                grant: b().grant,
                resource: resource(),
                ordinal: 0,
                offset: 0,
                bytes: vec![1, 2, 3, 4],
            }),
        )
        .unwrap();
    let tail = c
        .enqueue_native_input_response(tx(82), ack(), None)
        .unwrap();
    refuse(&mut c, &mut p, a.first);
    drive(&mut c, &mut p, |c, _| {
        c.custody(payload.first) == Some(Custody::DroppedUnsent)
    })
    .unwrap();
    commit(&mut c, &mut p, tail.first);
    assert!(p.uploads.is_empty());
}

fn scene_ready() -> (ShellConnection, Peer, NativeLauncherLifecycle, NativeScene) {
    let (mut c, mut p, mut l) = setup();
    let a = c.allocate_native(&mut l, tx(70), params(), 900).unwrap();
    commit(&mut c, &mut p, a.first);
    apply_allocation(&mut l, 1, 1);
    let a = c.begin_native_resource(&mut l, tx(80), begin()).unwrap();
    commit(&mut c, &mut p, a.first);
    status(&mut c, &mut p, &mut l, 1);
    let a = c.upload_native_chunk(&mut l, &[1, 2, 3, 4]).unwrap();
    drive(&mut c, &mut p, |c, _| {
        c.custody(a.first) == Some(Custody::Stored)
    })
    .unwrap();
    let a = c.end_native_resource(&mut l, tx(81)).unwrap();
    commit(&mut c, &mut p, a.first);
    status(&mut c, &mut p, &mut l, 2);
    let mut value = candidate().candidate;
    value.candidate.placements[0].resource = resource();
    let scene = NativeScene {
        placements: value.candidate.placements,
        targets: value.candidate.targets,
        rows: value.rows,
        selected: value.selected,
    };
    (c, p, l, scene)
}
fn permit(l: &mut NativeLauncherLifecycle, id: u64, state: u16) {
    l.observe(&NativeLauncherObservation::Content(
        tx(90),
        ShellContentRecord::FramePermit(ContentFramePermit {
            grant: b().grant,
            output: b().output,
            demand_id: id,
            permit_id: 77,
            state,
            reason: if state == 1 { 0 } else { 6 },
            ttl_ms: if state == 1 { 100 } else { 0 },
            max_candidate_bytes: 8192,
        }),
    ))
    .unwrap();
}

#[test]
fn local_permit_expiry_waits_for_authoritative_end() {
    let (mut c, mut p, mut l, scene) = scene_ready();
    let a = c.demand_native_frame(&mut l, tx(90), 1, 1000).unwrap();
    commit(&mut c, &mut p, a.first);
    permit(&mut l, 1, 1);
    assert_eq!(l.permit_deadline_ms(), Some(1100));
    assert!(
        c.present_native_scene(&mut l, tx(91), &scene, 1095, 10)
            .is_err()
    );
    l.service_at(&c, 1100).unwrap();
    assert_eq!(l.permit_deadline_ms(), None);
    assert!(
        c.present_native_scene(&mut l, tx(91), &scene, 1050, 10)
            .is_err()
    );
    assert!(c.cancel_native_demand(&mut l, tx(92), 1050, 10).is_err());
    assert!(c.demand_native_frame(&mut l, tx(93), 1, 1200).is_err());
    permit(&mut l, 1, 2);
    c.demand_native_frame(&mut l, tx(93), 1, 1200).unwrap();
}

#[test]
fn expiry_and_candidate_refusal_work_in_either_observation_order() {
    for expiry_first in [false, true] {
        let (mut c, mut p, mut l, scene) = scene_ready();
        let a = c.demand_native_frame(&mut l, tx(90), 1, 1000).unwrap();
        commit(&mut c, &mut p, a.first);
        permit(&mut l, 1, 1);
        let a = c
            .present_native_scene(&mut l, tx(91), &scene, 1050, 10)
            .unwrap();
        assert!(c.demand_native_frame(&mut l, tx(92), 1, 1060).is_err());
        if expiry_first {
            permit(&mut l, 1, 2);
        }
        refuse(&mut c, &mut p, a.first);
        l.service_at(&c, 1200).unwrap();
        if !expiry_first {
            assert!(c.demand_native_frame(&mut l, tx(92), 1, 1200).is_err());
            permit(&mut l, 1, 2);
        }
        let a = c.demand_native_frame(&mut l, tx(92), 1, 1200).unwrap();
        let wire = commit(&mut c, &mut p, a.first);
        let record = decode_shell_file_transaction(&wire, ShellFileKind::FrameDemand)
            .unwrap()
            .record;
        let ShellContentRecord::FrameDemand(v) = record else {
            panic!("demand")
        };
        assert_eq!(v.demand_id, 2);
        permit(&mut l, 2, 1);
        permit(&mut l, 1, 2); // old end must not disarm the new permit
        assert_eq!(l.permit_deadline_ms(), Some(1300));
    }
}

#[test]
fn scene_uses_owned_resources_and_consumes_permit_only_on_admission() {
    let (mut c, mut p, mut l, mut scene) = scene_ready();
    let a = c.demand_native_frame(&mut l, tx(90), 1, 1000).unwrap();
    commit(&mut c, &mut p, a.first);
    permit(&mut l, 1, 1);
    scene.placements[0].resource.generation += 1;
    assert!(
        c.present_native_scene(&mut l, tx(91), &scene, 1050, 10)
            .is_err()
    );
    assert_eq!(l.permit_deadline_ms(), Some(1100));
    scene.placements[0].resource = resource();
    let a = c
        .present_native_scene(&mut l, tx(91), &scene, 1050, 10)
        .unwrap();
    let wire = commit(&mut c, &mut p, a.first);
    let value = decode_shell_file_native_candidate(&wire).unwrap();
    assert_eq!(value.candidate.candidate.pacing_permit, 77);
    assert_eq!(
        value.candidate.candidate.surfaces[0].allocation,
        b().allocation
    );
    assert_eq!(value.candidate.candidate.interaction_generation, 1);
    assert!(
        c.present_native_scene(&mut l, tx(92), &scene, 1051, 10)
            .is_err()
    );
    let result = |kind| {
        NativeLauncherObservation::Content(
            tx(91),
            ShellContentRecord::CandidateOutcome(ContentCandidateOutcome {
                grant: b().grant,
                candidate_generation: 1,
                output: b().output,
                kind,
                reason: 0,
                presentation_epoch: if kind == 2 { 8 } else { 0 },
                work_area_generation: 1,
                wm_commit_generation: 1,
            }),
        )
    };
    l.observe(&result(1)).unwrap();
    assert!(c.demand_native_frame(&mut l, tx(92), 1, 1052).is_err());
    l.observe(&result(2)).unwrap();
    permit(&mut l, 1, 2);
    let a = c.demand_native_frame(&mut l, tx(92), 1, 1200).unwrap();
    commit(&mut c, &mut p, a.first);
    permit(&mut l, 2, 1);
    let a = c.cancel_native_demand(&mut l, tx(93), 1250, 10).unwrap();
    commit(&mut c, &mut p, a.first);
    assert!(
        c.present_native_scene(&mut l, tx(94), &scene, 1251, 10)
            .is_err()
    );
    assert!(c.demand_native_frame(&mut l, tx(94), 1, 1251).is_err());
    permit(&mut l, 2, 3);
    c.demand_native_frame(&mut l, tx(94), 1, 1300).unwrap();
}

fn params() -> NativeAllocationParams {
    NativeAllocationParams {
        edge: 2,
        width: 100,
        height: 50,
        margins: ContentMargins::default(),
    }
}
fn allocated(request: u64, status: u16) -> ContentAllocationResult {
    ContentAllocationResult {
        grant: b().grant,
        allocation_request_id: request,
        status,
        reason: if status == 1 { 0 } else { 1 },
        output: b().output,
        allocation: b().allocation,
        parent: ContentAllocationId::default(),
        scale_generation: 1,
        logical: ContentLogicalRect {
            x: 0,
            y: 0,
            width: 100,
            height: 50,
        },
        pixel: ContentPixelRect {
            x: 0,
            y: 0,
            width: 100,
            height: 50,
        },
        scale_numerator: 1,
        scale_denominator: 1,
        allowed_reservation_extent: 0,
        margins: ContentMargins::default(),
        acknowledged_anchor: ContentPixelRect::default(),
    }
}
fn apply_allocation(l: &mut NativeLauncherLifecycle, request: u64, status: u16) {
    l.observe(&NativeLauncherObservation::Content(
        tx(70),
        ShellContentRecord::AllocationResult(allocated(request, status)),
    ))
    .unwrap();
}
fn resource() -> ContentResourceId {
    ContentResourceId {
        id: 77,
        generation: 1,
    }
}
fn begin() -> ContentResourceBegin {
    ContentResourceBegin {
        grant: b().grant,
        resource: resource(),
        width_px: 1,
        height_px: 1,
        rendered_scale_numerator: 1,
        rendered_scale_denominator: 1,
        pixel_format: 1,
        chunk_count: 1,
        total_bytes: 4,
    }
}
fn status(c: &mut ShellConnection, p: &mut Peer, l: &mut NativeLauncherLifecycle, state: u16) {
    let body = encode_shell_file_resource_status_body(&ShellFileTransactionRecord {
        transaction: tx(80),
        record: ShellContentRecord::ResourceStatus(ContentResourceStatus {
            grant: b().grant,
            resource: resource(),
            status: state,
            reason: if state <= 2 { 0 } else { 11 },
            next_ordinal: if state == 2 { 1 } else { 0 },
            admitted_bytes: if state == 2 { 4 } else { 0 },
        }),
    })
    .unwrap();
    let event = p.event(ShellFileKind::ResourceStatus, &body);
    p.push(event);
    drive(c, p, |c, _| {
        if let Some(v) = c.take_native_launcher_observation().unwrap() {
            l.observe(&v).unwrap();
            true
        } else {
            false
        }
    })
    .unwrap();
}
fn refuse(c: &mut ShellConnection, p: &mut Peer, ticket: Ticket) {
    drive(c, p, |_, p| !p.submits.is_empty()).unwrap();
    let held = p.submits.pop_front().unwrap();
    p.answer_error(held.tag, 22);
    drive(c, p, |c, _| c.custody(ticket) == Some(Custody::Refused(22))).unwrap();
}

#[test]
fn allocation_resize_rejection_close_and_invalidation() {
    let (mut c, mut p, mut l) = setup();
    let a = c.allocate_native(&mut l, tx(70), params(), 100).unwrap();
    assert!(c.allocate_native(&mut l, tx(71), params(), 100).is_err());
    let wire = commit(&mut c, &mut p, a.first);
    let record = decode_shell_file_native_launcher_transaction(
        &wire,
        ShellFileKind::NativeAllocationRequest,
    )
    .unwrap();
    let ShellNativeLauncherRecord::AllocationRequest(request) = record.record else {
        panic!("request")
    };
    assert_eq!((request.operation, request.request_id), (1, 1));
    assert_eq!(
        l.allocation_deadline_ms(),
        Some(100 + u64::from(ContentLimits::prototype(b().grant).allocation_timeout_ms))
    );
    apply_allocation(&mut l, 1, 1);
    assert_eq!(l.allocation(), Some(&allocated(1, 1)));
    let a = c.allocate_native(&mut l, tx(71), params(), 200).unwrap();
    let wire = commit(&mut c, &mut p, a.first);
    let ShellNativeLauncherRecord::AllocationRequest(request) =
        decode_shell_file_native_launcher_transaction(
            &wire,
            ShellFileKind::NativeAllocationRequest,
        )
        .unwrap()
        .record
    else {
        panic!("request")
    };
    assert_eq!(
        (request.operation, request.request_id, request.prior),
        (2, 2, b().allocation)
    );
    apply_allocation(&mut l, 2, 2);
    assert_eq!(l.allocation(), Some(&allocated(1, 1)));
    observe(
        &mut l,
        ShellNativeLauncherRecord::Closed(NativeLauncherClosed {
            grant: b().grant,
            opening: 1,
            reason: 11,
        }),
    );
    assert!(l.allocation().is_none() && l.awaiting_invalidation());
    observe(
        &mut l,
        ShellNativeLauncherRecord::Opening(NativeLauncherOpening {
            grant: b().grant,
            opening: 2,
            output: b().output,
            catalog_generation: 6,
            state_revision: 1,
        }),
    );
    assert!(c.allocate_native(&mut l, tx(72), params(), 300).is_err());
    apply_allocation(&mut l, 0, 4);
    assert!(!l.awaiting_invalidation());
    let a = c.allocate_native(&mut l, tx(72), params(), 300).unwrap();
    refuse(&mut c, &mut p, a.first);
    l.service(&c).unwrap();
    assert!(l.allocation_deadline_ms().is_none());
    c.allocate_native(&mut l, tx(73), params(), 400).unwrap();
}

#[test]
fn late_allocation_grant_remains_orphaned_until_invalidation() {
    let (mut c, _p, mut l) = setup();
    c.allocate_native(&mut l, tx(70), params(), 100).unwrap();
    observe(
        &mut l,
        ShellNativeLauncherRecord::Closed(NativeLauncherClosed {
            grant: b().grant,
            opening: 1,
            reason: 11,
        }),
    );
    apply_allocation(&mut l, 1, 1);
    assert!(l.allocation().is_none() && l.awaiting_invalidation());
    assert!(c.release_native_allocation(&mut l, tx(71), 200).is_err());
}

#[test]
fn resource_upload_waits_for_stored_and_survives_close_until_release() {
    let (mut c, mut p, mut l) = setup();
    let a = c.begin_native_resource(&mut l, tx(80), begin()).unwrap();
    assert_eq!(
        l.resource_state(resource()),
        Some(NativeResourceState::Begun)
    );
    assert!(c.begin_native_resource(&mut l, tx(81), begin()).is_err());
    assert!(c.upload_native_chunk(&mut l, &[1, 2]).is_err());
    commit(&mut c, &mut p, a.first);
    status(&mut c, &mut p, &mut l, 1);
    assert!(c.end_native_resource(&mut l, tx(81)).is_err());
    // Two caller writes for one declared layout chunk; transport can split further.
    for bytes in [&[1, 2][..], &[3, 4][..]] {
        let a = c.upload_native_chunk(&mut l, bytes).unwrap();
        assert!(matches!(
            c.upload_native_chunk(&mut l, bytes),
            Err(ShellClientError::QueueSaturated)
        ));
        drive(&mut c, &mut p, |c, _| {
            c.custody(a.first) == Some(Custody::Stored)
        })
        .unwrap();
        l.service(&c).unwrap();
    }
    assert_eq!(p.uploads.get("upload/0").unwrap(), &[1, 2, 3, 4]);
    let a = c.end_native_resource(&mut l, tx(81)).unwrap();
    assert_eq!(
        l.resource_state(resource()),
        Some(NativeResourceState::Admitted)
    );
    commit(&mut c, &mut p, a.first);
    status(&mut c, &mut p, &mut l, 2);
    assert_eq!(
        l.resource_state(resource()),
        Some(NativeResourceState::Accepted)
    );
    observe(
        &mut l,
        ShellNativeLauncherRecord::Closed(NativeLauncherClosed {
            grant: b().grant,
            opening: 1,
            reason: 11,
        }),
    );
    let a = c
        .retire_native_resource(&mut l, tx(82), resource())
        .unwrap();
    assert_eq!(
        l.resource_state(resource()),
        Some(NativeResourceState::Retiring)
    );
    refuse(&mut c, &mut p, a.first);
    l.service(&c).unwrap();
    assert_eq!(
        l.resource_state(resource()),
        Some(NativeResourceState::Accepted)
    );
    let a = c
        .retire_native_resource(&mut l, tx(83), resource())
        .unwrap();
    commit(&mut c, &mut p, a.first);
    l.observe(&NativeLauncherObservation::Content(
        tx(83),
        ShellContentRecord::ResourceReleased(ContentResourceReleased {
            grant: b().grant,
            resource: resource(),
            reason: 1,
        }),
    ))
    .unwrap();
    assert_eq!(l.resource_state(resource()), None);
}

#[test]
fn refused_payload_cannot_end_or_replay_and_can_be_cancelled() {
    let (mut c, mut p, mut l) = setup();
    let a = c.begin_native_resource(&mut l, tx(80), begin()).unwrap();
    commit(&mut c, &mut p, a.first);
    status(&mut c, &mut p, &mut l, 1);
    p.upload_replies.push_back(Err(22));
    let a = c.upload_native_chunk(&mut l, &[1, 2, 3, 4]).unwrap();
    drive(&mut c, &mut p, |c, _| {
        c.custody(a.first) == Some(Custody::Refused(22))
    })
    .unwrap();
    l.service(&c).unwrap();
    assert!(c.end_native_resource(&mut l, tx(81)).is_err());
    assert!(c.upload_native_chunk(&mut l, &[1, 2, 3, 4]).is_err());
    let a = c
        .cancel_native_resource(&mut l, tx(82), resource())
        .unwrap();
    commit(&mut c, &mut p, a.first);
    status(&mut c, &mut p, &mut l, 4);
    assert_eq!(l.resource_state(resource()), None);
}

#[test]
fn refused_end_can_be_cancelled_by_resource_identity() {
    let (mut c, mut p, mut l) = setup();
    let a = c.begin_native_resource(&mut l, tx(80), begin()).unwrap();
    commit(&mut c, &mut p, a.first);
    status(&mut c, &mut p, &mut l, 1);
    let a = c.upload_native_chunk(&mut l, &[1, 2, 3, 4]).unwrap();
    drive(&mut c, &mut p, |c, _| {
        c.custody(a.first) == Some(Custody::Stored)
    })
    .unwrap();
    let a = c.end_native_resource(&mut l, tx(81)).unwrap();
    refuse(&mut c, &mut p, a.first);
    l.service(&c).unwrap();
    let a = c
        .cancel_native_resource(&mut l, tx(82), resource())
        .unwrap();
    assert_eq!(
        l.resource_state(resource()),
        Some(NativeResourceState::Admitted)
    );
    assert!(c.begin_native_resource(&mut l, tx(83), begin()).is_err());
    commit(&mut c, &mut p, a.first);
    status(&mut c, &mut p, &mut l, 4);
    assert_eq!(l.resource_state(resource()), None);
    assert!(
        c.cancel_native_resource(&mut l, tx(83), resource())
            .is_err()
    );
    c.begin_native_resource(&mut l, tx(84), begin()).unwrap();
}
