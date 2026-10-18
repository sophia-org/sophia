use super::*;

struct ImagePeer {
    _owner: std::os::fd::OwnedFd,
    dir: std::path::PathBuf,
}
impl Drop for ImagePeer {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
fn image_ready() -> (ShellConnection, Peer, NativeLauncherLifecycle, ImagePeer) {
    use crate::image_peer as image;
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let name = format!("native-scene-{}", NEXT.fetch_add(1, Ordering::Relaxed));
    let (c, p, dir, listener) = image::connect_role_image(
        &name,
        4096,
        Profile {
            role: "launcher",
            revision: 9,
            capabilities: CAPS | image::SEALED,
            limits_published: true,
        },
        false,
    );
    let (mut c, mut p, mut l) = setup_with((c, p));
    let owner = image::drive_to_ready(&mut c, &mut p, &listener);
    let mut m = image::metadata();
    m.resource = resource().id;
    m.admission_epoch = b().grant.content_grant_epoch;
    c.image_begin(m, image::sealed_fd(), Instant::now())
        .unwrap();
    image::answer_submit(&mut c, &mut p, ShellFileKind::ImageBegin);
    let m = ShellImageMetadata::decode(&p.transactions.last().unwrap()[32..]).unwrap();
    let event = p.event_image(ShellFileKind::ImageAdmitted, &m.encode().unwrap());
    p.push(event);
    image::drive(&mut c, &mut p, |c, _| {
        c.image_adapter().unwrap().offer(m.offer_id).unwrap().state == OfferState::Sent
    });
    let (_, fds) = image::recv_packet(&owner);
    assert_eq!(fds.len(), 1);
    let body = encode_shell_file_resource_status_body(&ShellFileTransactionRecord {
        transaction: tx(m.offer_id),
        record: ShellContentRecord::ResourceStatus(ContentResourceStatus {
            grant: b().grant,
            resource: resource(),
            status: 2,
            reason: 0,
            next_ordinal: 0,
            admitted_bytes: m.declared_bytes,
        }),
    })
    .unwrap();
    let event = p.event(ShellFileKind::ResourceStatus, &body);
    p.push(event);
    let body = ShellSourceReleased {
        offer_id: m.offer_id,
        resource: m.resource,
        resource_generation: m.resource_generation,
        disposition: ShellSourceDisposition::Retained,
    }
    .encode()
    .unwrap();
    let event = p.event_image(ShellFileKind::SourceReleased, &body);
    p.push(event);
    image::drive(&mut c, &mut p, |c, _| {
        c.image_adapter().unwrap().offer(m.offer_id).unwrap().state == OfferState::Settled
    });
    assert_eq!(
        c.adopt_native_image(&mut l, m.offer_id).unwrap(),
        resource()
    );
    while let Some(value) = c.take_native_launcher_observation().unwrap() {
        l.observe(&value).unwrap();
    }
    (c, p, l, ImagePeer { _owner: owner, dir })
}
fn scene_ready() -> (
    ShellConnection,
    Peer,
    NativeLauncherLifecycle,
    NativeScene,
    ImagePeer,
) {
    let (mut c, mut p, mut l, image) = image_ready();
    let a = c.allocate_native(&mut l, tx(70), params(), 900).unwrap();
    commit(&mut c, &mut p, a.first);
    apply_allocation(&mut l, 1, 1);
    let mut value = candidate().candidate;
    value.candidate.placements[0].resource = resource();
    let scene = NativeScene {
        placements: value.candidate.placements,
        targets: value.candidate.targets,
        rows: value.rows,
        selected: value.selected,
    };
    (c, p, l, scene, image)
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
    let (mut c, mut p, mut l, scene, _image) = scene_ready();
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
        let (mut c, mut p, mut l, scene, _image) = scene_ready();
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
    let (mut c, mut p, mut l, mut scene, _image) = scene_ready();
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
fn adopted_resource_survives_close_until_authoritative_release() {
    let (mut c, mut p, mut l, _image) = image_ready();
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
