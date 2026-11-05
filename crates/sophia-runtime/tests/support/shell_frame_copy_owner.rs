use super::*;
use crate::shell_transport::{
    ShellComponentTransport, files::ShellFileWire, owned_copy::OwnedCopyOwner,
};
use crate::{ContentAllocationSnapshot, ContentCandidateContext, IsbValidationWorker};
use sophia_protocol::*;
use std::fs::File;
use std::os::unix::{fs::FileExt, net::UnixStream};

struct Fixture {
    transport: ShellComponentTransport,
    epochs: ContentEpochRegistry,
    owner: OwnedCopyOwner,
    worker: IsbValidationWorker,
    _peer: UnixStream,
}

impl Fixture {
    fn new() -> Self {
        let mut epochs = epochs();
        let mut request = offer(1, 0, 2).request;
        request.allocation.generation = 1;
        let allocations = epochs.allocations_mut(identity().grant).unwrap();
        allocations
            .publish_outputs(
                TransactionId::from_raw(1),
                12,
                vec![ContentOutputFactsEntry {
                    output: request.output,
                    local_width: 48,
                    local_height: 100,
                    scale_numerator: 1,
                    scale_denominator: 1,
                    scale_generation: 1,
                }],
            )
            .unwrap();
        allocations
            .request(
                TransactionId::from_raw(2),
                ContentAllocationRequest {
                    grant: identity().grant,
                    output: request.output,
                    allocation_request_id: 1,
                    operation: 1,
                    role: 1,
                    edge: 1,
                    prior: ContentAllocationId::default(),
                    parent: ContentAllocationId::default(),
                    parent_presentation_epoch: 0,
                    anchor_parent_rect: ContentPixelRect::default(),
                    desired_width: 48,
                    desired_height: 24,
                    margins: ContentMargins::default(),
                },
                &[],
                0,
            )
            .unwrap();
        allocations
            .grant(
                1,
                ContentAllocationSnapshot {
                    native_opening: None,
                    output: request.output,
                    allocation: request.allocation,
                    scale_generation: 1,
                    scale_numerator: 1,
                    scale_denominator: 1,
                    role: 1,
                    edge: 1,
                    margins: ContentMargins::default(),
                    logical: ContentLogicalRect {
                        x: 0,
                        y: 0,
                        width: 48,
                        height: 24,
                    },
                    pixel: ContentPixelRect {
                        x: 0,
                        y: 0,
                        width: 48,
                        height: 24,
                    },
                    parent: ContentAllocationId::default(),
                    anchor_parent_rect: ContentPixelRect::default(),
                    allowed_reservation_extent: 24,
                },
                &[],
            )
            .unwrap();
        while allocations.take_event().is_some() {}
        let allocations = allocations.snapshots();
        let store = epochs.active_candidates_mut(identity().grant).unwrap();
        store
            .open_frame_stream(
                TransactionId::from_raw(3),
                crate::frame_credits::FrameStreamKey {
                    grant: identity().grant,
                    output: request.output,
                    allocation: request.allocation,
                    facts_generation: 12,
                    stream_id: 1,
                },
                1,
                ContentCandidateContext {
                    output: request.output,
                    facts_generation: 12,
                    interaction_generation: 1,
                    allocations: &allocations,
                },
                0,
            )
            .unwrap();
        store.take_frame_stream_event().unwrap();
        let mut files = export();
        register(&mut files);
        assert!(
            files
                .service_owned_ring_setup(&mut epochs, Instant::now())
                .unwrap()
                .is_none()
        );
        let source = File::from(ring_fd(true));
        source.write_all_at(&2_u64.to_le_bytes(), 0).unwrap();
        files
            .receive_owned_source(packet(&registration().source_packet(), &[source.as_fd()]))
            .unwrap();
        let (local, peer) = UnixStream::pair().unwrap();
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "frame-copy-owner-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let mut transport = ShellComponentTransport::bind_for_supervised_uid(
            path,
            rustix::process::geteuid().as_raw(),
        )
        .unwrap();
        transport.connection_epoch = 1;
        transport.store_grant = identity().grant;
        transport.content_grant = Some(identity().grant);
        transport.content_limits = Some(ContentLimits::prototype(identity().grant));
        transport.capabilities = CAPS;
        transport.wire = Some(Box::new(
            ShellFileWire::adopt(local, files, None, false).unwrap(),
        ));
        let mut fixture = Self {
            transport,
            epochs,
            owner: OwnedCopyOwner::default(),
            worker: IsbValidationWorker::new(1, PIXELS).unwrap(),
            _peer: peer,
        };
        fixture.service();
        fixture
    }

    fn service(&mut self) {
        self.owner
            .service(
                &mut self.transport,
                &mut self.epochs,
                &mut self.worker,
                1,
                Instant::now(),
                0,
            )
            .unwrap();
    }

    fn submit(&mut self, stream: u64) {
        let ShellFrameRecord::Commit(mut frame) =
            decode_shell_frame_record(&golden("commit-copy"), ShellFileClass::Candidate, 13, 8192)
                .unwrap()
        else {
            panic!()
        };
        let mut request = offer(1, 0, 2).request;
        request.allocation.generation = 1;
        frame.candidate.candidate_generation = 1;
        frame.candidate.output = request.output;
        frame.candidate.facts_generation = 12;
        frame.candidate.pacing_permit = stream;
        frame.candidate.interaction_generation = 1;
        frame.candidate.surfaces[0].allocation = request.allocation;
        frame.candidate.surfaces[0].scale_generation = 1;
        frame.candidate.surfaces[0].reservation_extent = 24;
        frame.candidate.placements[0].resource = request.resource;
        frame.image = Some(RingOffer {
            identity: RingIdentity {
                connection_epoch: 1,
                content_grant_epoch: 2,
                channel_generation: 1,
                ring: 7,
                generation: 1,
            },
            offer_id: 1,
            slot: 0,
            slot_generation: 2,
            resource: request.resource.id,
            resource_generation: request.resource.generation,
            allocation: request.allocation.id,
            allocation_generation: request.allocation.generation,
            output: request.output.id,
            output_generation: request.output.generation,
            topology_generation: 12,
            width: 48,
            height: 24,
        });
        let record = ShellFrameRecord::Commit(frame);
        let bytes = encode_shell_frame_record(
            ShellFileHeader {
                kind: record.kind(),
                connection_epoch: 1,
                submission_id: 12,
                sequence: 0,
            },
            &record,
            13,
        )
        .unwrap();
        let files = self.transport.files_mut().unwrap().export_mut();
        let (_writer, submit) = stage(files, &bytes);
        files.submit_transaction(&submit).unwrap();
    }
}

#[test]
fn complete_file_commit_drives_private_copy_then_status_before_slot_release() {
    let mut f = Fixture::new();
    f.submit(1);
    f.service();
    assert_eq!(f.owner.pending(), 1);
    assert_eq!(f.worker.pending(), 1);
    assert!(
        f.transport
            .files_mut()
            .unwrap()
            .export()
            .peek_frame_request()
            .is_none()
    );
    let resource = offer(1, 0, 2).request.resource;
    assert!(
        f.epochs
            .resources(identity().grant)
            .unwrap()
            .lease(identity().grant, resource)
            .is_err()
    );
    assert!(
        f.epochs
            .active_candidates(identity().grant)
            .unwrap()
            .ready_frame_commit()
            .is_none()
    );
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(completion) = f.worker.take_completed().unwrap() {
            assert!(f.owner.receive(completion).is_ok());
            break;
        }
        assert!(Instant::now() < until);
        std::thread::yield_now();
    }
    f.service();
    assert_eq!(f.worker.pending(), 0);
    assert_eq!(
        f.owner.pending(),
        1,
        "release waits for ResourceStatus publication"
    );
    assert!(
        f.epochs
            .active_candidates(identity().grant)
            .unwrap()
            .ready_frame_commit()
            .is_some()
    );
    assert!(
        f.epochs
            .resources(identity().grant)
            .unwrap()
            .lease(identity().grant, resource)
            .is_err(),
        "copy is private until Presented"
    );
    f.transport
        .flush_content_resource_events(&mut f.epochs)
        .unwrap();
    crate::shell_transport::outbox::tests::files::Peer::drain(&mut f.transport);
    f.service();
    assert_eq!(f.owner.pending(), 0);
    let files = f.transport.files_mut().unwrap().export();
    assert!(files.owned_pending_offer().is_none());
    assert!(files.owned.live.iter().all(Option::is_none));
    assert_eq!(journal_statuses(files).len(), 1);
    let released = journal_events(files)
        .into_iter()
        .find(|event| event.kind == EventKind::SlotReleased)
        .unwrap();
    assert_eq!(released.slot, offer(1, 0, 2).request.slot);
}

#[test]
fn legacy_source_admission_requires_the_stream_close_to_drain() {
    for phase in 0..3 {
        let mut f = Fixture::new();
        let mut request = offer(1, 0, 2);
        request.request.allocation.generation = 1;
        if phase != 0 {
            let store = f.epochs.active_candidates_mut(identity().grant).unwrap();
            store
                .close_frame_stream(
                    TransactionId::from_raw(4),
                    crate::frame_credits::FrameStreamKey {
                        grant: identity().grant,
                        output: request.request.output,
                        allocation: request.request.allocation,
                        facts_generation: 12,
                        stream_id: 1,
                    },
                    1,
                )
                .unwrap();
            if phase == 2 {
                store.take_frame_stream_event().unwrap();
            }
        }
        f.transport
            .files_mut()
            .unwrap()
            .export_mut()
            .write_owned_ring(&request.encode())
            .unwrap();
        f.service();
        if phase != 2 {
            assert_eq!(
                f.worker.pending(),
                0,
                "legacy copy admitted in phase {phase}"
            );
            assert_eq!(f.owner.pending(), 0);
            let files = f.transport.files_mut().unwrap().export();
            assert!(files.owned_pending_offer().is_none());
            assert!(files.owned.live.iter().all(Option::is_none));
            assert_eq!(journal_statuses(files).len(), 1);
            let release = journal_events(files)
                .into_iter()
                .find(|event| event.kind == EventKind::SlotReleased)
                .unwrap();
            assert_eq!(release.slot, request.request.slot);
            assert_eq!(release.reason, ContentReason::Stale as u16);
        } else {
            assert_eq!(f.worker.pending(), 1, "drained stream permits legacy copy");
            let until = Instant::now() + Duration::from_secs(5);
            loop {
                if let Some(completion) = f.worker.take_completed().unwrap() {
                    assert!(f.owner.receive(completion).is_ok());
                    break;
                }
                assert!(Instant::now() < until);
                std::thread::yield_now();
            }
            f.service();
            assert!(
                f.epochs
                    .resources(identity().grant)
                    .unwrap()
                    .lease(identity().grant, request.request.resource)
                    .is_ok()
            );
        }
    }
}

#[test]
fn refused_stream_key_cancels_source_reservations_without_reading_or_spending_offer() {
    let mut f = Fixture::new();
    f.submit(2);
    f.service();
    assert_eq!(f.owner.pending(), 0);
    assert_eq!(f.worker.pending(), 0);
    assert_eq!(
        f.epochs
            .resources(identity().grant)
            .unwrap()
            .usage()
            .backing,
        0
    );
    let files = f.transport.files_mut().unwrap().export_mut();
    assert!(files.peek_frame_request().is_none());
    assert!(files.owned.live.iter().all(Option::is_none));
    let held = files.reserve_owned_offer(offer(1, 0, 2)).unwrap().unwrap();
    files.cancel_owned_offer(held);
    assert!(
        matches!(f.transport.output.front().unwrap().record, crate::shell_transport::outbound::OutboundRecord::Frame(ShellFrameRecord::Refused { reason, .. }) if reason == ContentReason::Stale as u16)
    );
}

#[test]
fn owner_backpressure_retains_whole_commit_without_reserving_source_or_worker() {
    let mut f = Fixture::new();
    f.submit(1);
    let occupied = f.epochs.control_occupancy(identity().grant) as u32;
    f.transport
        .content_limits
        .as_mut()
        .unwrap()
        .max_control_records = occupied;
    f.service();
    assert_eq!(f.owner.pending(), 0);
    assert_eq!(f.worker.pending(), 0);
    let files = f.transport.files_mut().unwrap().export();
    assert!(files.peek_frame_request().is_some());
    assert!(files.owned.live.iter().all(Option::is_none));
    f.transport
        .content_limits
        .as_mut()
        .unwrap()
        .max_control_records = 128;
    f.service();
    assert_eq!(f.owner.pending(), 1);
}

#[test]
fn source_budget_refusal_consumes_only_the_frame_identity() {
    let mut f = Fixture::new();
    f.submit(1);
    let occupied = f
        .worker
        .reserve(PIXELS, Instant::now() + Duration::from_secs(5))
        .unwrap();
    f.service();
    assert_eq!(f.owner.pending(), 0);
    assert_eq!(
        f.worker.pending(),
        1,
        "only the unrelated reservation remains"
    );
    assert_eq!(
        f.epochs
            .resources(identity().grant)
            .unwrap()
            .usage()
            .backing,
        0
    );
    let files = f.transport.files_mut().unwrap().export_mut();
    assert!(files.peek_frame_request().is_none());
    assert!(files.owned.live.iter().all(Option::is_none));
    let held = files.reserve_owned_offer(offer(1, 0, 2)).unwrap().unwrap();
    files.cancel_owned_offer(held);
    assert!(
        matches!(f.transport.output.front().unwrap().record, crate::shell_transport::outbound::OutboundRecord::Frame(ShellFrameRecord::Refused { reason, candidate_generation: 1, .. }) if reason == ContentReason::Budget as u16)
    );
    assert!(f.worker.release_reserved(occupied));
}

#[test]
fn busy_source_is_a_typed_refusal_not_a_blocked_input_head() {
    let mut f = Fixture::new();
    f.submit(1);
    let held = f
        .transport
        .files_mut()
        .unwrap()
        .export_mut()
        .reserve_owned_offer(offer(1, 0, 2))
        .unwrap()
        .unwrap();
    f.service();
    assert_eq!(f.owner.pending(), 0);
    assert_eq!(f.worker.pending(), 0);
    assert!(
        f.transport
            .files_mut()
            .unwrap()
            .export()
            .peek_frame_request()
            .is_none()
    );
    assert!(
        matches!(f.transport.output.front().unwrap().record, crate::shell_transport::outbound::OutboundRecord::Frame(ShellFrameRecord::Refused { reason, .. }) if reason == ContentReason::Budget as u16)
    );
    f.transport
        .files_mut()
        .unwrap()
        .export_mut()
        .cancel_owned_offer(held);
}
