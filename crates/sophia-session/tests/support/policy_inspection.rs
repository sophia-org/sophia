use super::*;
use sophia_9p::client::{Client, ClientLimits, File};
use sophia_protocol::inspection::*;

fn attach(public: &LivePublicPolicyState) -> (Client, File) {
    let path = public.inspection.as_ref().unwrap().service.socket_path();
    let mut client = Client::connect(path, ClientLimits::default()).unwrap();
    let root = client.attach(b"", b"").unwrap();
    (client, root)
}

fn snapshot(client: &mut Client, root: &File) -> InspectionSnapshotRecord {
    let mut file = client.walk(root, &[b"snapshot"]).unwrap();
    client.open(&mut file, false).unwrap();
    let bytes = client
        .read_to_end(&file, INSPECTION_MAX_SNAPSHOT_BYTES)
        .unwrap();
    client.clunk(file).unwrap();
    decode_inspection_snapshot(&bytes).unwrap()
}

fn flush_observation(public: &mut LivePublicPolicyState) {
    let deadline = Instant::now() + Duration::from_secs(3);
    public.publish_inspection();
    while public.inspection.as_ref().unwrap().retry {
        assert!(Instant::now() < deadline, "publication did not recover");
        public.publish_inspection();
        std::thread::yield_now();
    }
}

fn install(fixture: &mut ReloadFixture) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(
        &fixture.source.directory,
        std::fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    let mut service = InspectionService::bind(&fixture.source.directory).unwrap();
    service.fence(1, None).unwrap();
    fixture.wm.public.as_mut().unwrap().inspection = Some(LivePolicyInspection {
        publisher: service.publisher(),
        service,
        session_generation: 91,
        epoch: 1,
        excluded_pid: None,
        capabilities: Some(0),
        last: None,
        refused: None,
        pending_event: None,
        retry: true,
        refusals: 0,
    });
}

#[test]
fn safe_owner_view_and_reported_outcomes_do_not_depend_on_wm_wire() {
    // Owner-command evidence with a capture worker, not a real WM settlement
    // or a second reducer. The inspector reaches the real admitted service.
    for (wire, expected) in [
        (WmTransportSelection::CurrentIpc, InspectionWire::CurrentIpc),
        (WmTransportSelection::NineP2000L, InspectionWire::Files),
    ] {
        let mut fixture = ReloadFixture::new();
        install(&mut fixture);
        let public = fixture.wm.public.as_mut().unwrap();
        public.wm_transport = wire;
        public.selected_capabilities = 0x3dfff;
        public.inspection.as_mut().unwrap().capabilities = Some(0x3dfff);
        public
            .actions
            .push(sophia_protocol::PolicyActionRegistration {
                action: WmActionId::from_raw(7123456789),
                name: "DO_NOT_DISCLOSE_ACTION".into(),
                session_operation_slot: Some(57),
            });
        let mut scene = public.reducer.scene().clone();
        scene.generation += 1;
        scene.session_operations = vec![sophia_protocol::PolicySessionOperation {
            token: 9123456789012345678,
            slot: 57,
            permits_surface_target: true,
        }];
        public.reducer.observe_scene(scene).unwrap();
        public.note_inspection_event(Some(InspectionEvent::ConnectionChanged));
        flush_observation(public);
        let (mut client, root) = attach(public);
        let initial = snapshot(&mut client, &root);
        assert_eq!(initial.snapshot.wire, expected);
        assert_eq!(initial.snapshot.session_generation, 91);
        assert_eq!(initial.snapshot.selected_capabilities, 0x3dfff);
        let encoded = String::from_utf8(encode_inspection_snapshot(&initial).unwrap()).unwrap();
        for secret in [
            "DO_NOT_DISCLOSE_ACTION",
            "7123456789",
            "9123456789012345678",
            "session_operations",
            "policy_key",
        ] {
            assert!(!encoded.contains(secret), "leaked {secret}");
        }
        let mut events = client.walk(&root, &[b"events"]).unwrap();
        client.open(&mut events, false).unwrap();
        let (worker, commands, _event_sender) =
            policy_transport_worker::worker_capture::capturing_worker();
        public.worker = Some(worker);
        for request_id in 1..=80 {
            public
                .submit_or_defer(PolicyTransportCommand::ProjectionOutcome {
                    transaction: TransactionId::from_raw(100 + request_id),
                    request_id,
                    scene_generation: public.reducer.scene().generation,
                    outcome: sophia_protocol::PolicyProjectionOutcome::TimedOut,
                    expect_session_operation: false,
                })
                .unwrap();
            assert!(
                matches!(commands.try_recv().unwrap(), PolicyTransportCommand::ProjectionOutcome {
                request_id: id, outcome: sophia_protocol::PolicyProjectionOutcome::TimedOut,
                expect_session_operation: false, ..
            } if id == request_id)
            );
            // One explicit owner turn per outcome for this retention control.
            public.publish_inspection();
        }
        // The reader cannot hold the owner-command path or ring retention.
        flush_observation(public);
        assert!(client.read(&events, initial.event_offset, 4096).is_err());
        let (mut fresh, fresh_root) = attach(public);
        let current = snapshot(&mut fresh, &fresh_root);
        assert!(current.sequence > initial.sequence);
        assert!(
            current.sequence >= initial.sequence + 64
                || current.loss_generation > initial.loss_generation,
            "the watch must expose ring eviction or explicit publication loss"
        );
        assert_eq!(current.snapshot, initial.snapshot);
        public.fence_inspection(0, None);
        flush_observation(public);
        assert!(
            fresh.getattr(&fresh_root).is_err(),
            "old attachment survived replacement"
        );
        let (mut replacement, replacement_root) = attach(public);
        let unavailable = snapshot(&mut replacement, &replacement_root);
        assert_eq!(unavailable.snapshot.wm_epoch, 0);
        assert_eq!(unavailable.snapshot.state, InspectionState::Unavailable);
        assert!(unavailable.snapshot.outputs.is_empty());
        assert!(unavailable.generation > current.generation);
        public.fence_inspection(2, None);
        flush_observation(public);
        let (mut next, next_root) = attach(public);
        let starting = snapshot(&mut next, &next_root);
        assert_eq!(starting.snapshot.wm_epoch, 2);
        assert_eq!(starting.snapshot.selected_capabilities, 0);
        assert_eq!(starting.snapshot.state, InspectionState::Starting);
        // A supplied exclusion change exercises the fence identity without
        // claiming a protected child. Even with an unchanged scene/epoch it
        // must replace the cached observation before an idle owner can skip it.
        public.fence_inspection(2, Some(u32::MAX));
        flush_observation(public);
        assert!(next.getattr(&next_root).is_err());
        let (mut final_client, final_root) = attach(public);
        let renewed = snapshot(&mut final_client, &final_root);
        assert!(renewed.generation > starting.generation);
        assert_eq!(renewed.snapshot, starting.snapshot);
        fixture.wm.service_inspection(true);
        assert!(
            final_client.getattr(&final_root).is_err(),
            "stop retained observer authority"
        );
    }
}

#[test]
fn receipt_queue_reports_only_after_acceptance_and_terminal_drop_revokes() {
    let mut fixture = ReloadFixture::new();
    install(&mut fixture);
    let public = fixture.wm.public.as_mut().unwrap();
    flush_observation(public);
    let (mut client, root) = attach(public);
    let before = snapshot(&mut client, &root);
    let mut events = client.walk(&root, &[b"events"]).unwrap();
    client.open(&mut events, false).unwrap();
    let (worker, commands, _event_sender) =
        policy_transport_worker::worker_capture::capturing_worker();
    public.worker = Some(worker);
    // Supplied owner receipt: this test covers the reporting hook and never
    // claims that a physical frame retired to produce it.
    let receipt = sophia_protocol::PolicyPresentationReceipt {
        connection_epoch: 1,
        publication_generation: 23,
        output: sophia_protocol::OutputId::from_raw(1),
        output_generation: 17,
        presentation_epoch: 31,
        outcome: sophia_protocol::PolicyPresentationOutcome::Presented,
    };
    assert!(
        public
            .worker
            .as_ref()
            .unwrap()
            .try_command(PolicyTransportCommand::Stop)
            .is_ok()
    );
    public.presentation_receipts.push_back(receipt);
    assert!(!public.flush_presentation_receipts().unwrap());
    assert_eq!(public.presentation_receipts.len(), 1);
    assert_eq!(
        snapshot(&mut client, &root).sequence,
        before.sequence,
        "a refused receipt was reported as delivered to the command queue"
    );
    assert!(matches!(
        commands.try_recv().unwrap(),
        PolicyTransportCommand::Stop
    ));
    assert!(public.flush_presentation_receipts().unwrap());
    flush_observation(public);
    assert!(
        matches!(commands.try_recv().unwrap(), PolicyTransportCommand::PresentationReceipt { receipt: sent, .. } if sent == receipt)
    );
    let bytes = client.read(&events, before.event_offset, 4096).unwrap();
    let event = decode_inspection_event(&bytes).unwrap();
    assert_eq!(event.event, InspectionEvent::PresentationChanged);
    let text = String::from_utf8(bytes).unwrap();
    assert!(!text.contains("publication_generation"));
    assert!(!text.contains("presentation_epoch"));
    drop(fixture);
    assert!(
        client.getattr(&root).is_err(),
        "non-logout teardown retained inspection"
    );
}

#[test]
fn owner_turn_coalesces_reports_and_latches_permanent_record_refusal() {
    let mut fixture = ReloadFixture::new();
    install(&mut fixture);
    let public = fixture.wm.public.as_mut().unwrap();
    flush_observation(public);
    let (mut client, root) = attach(public);
    let before = snapshot(&mut client, &root);
    for _ in 0..80 {
        public.note_inspection_event(Some(InspectionEvent::PresentationChanged));
    }
    assert_eq!(snapshot(&mut client, &root).sequence, before.sequence);
    public.note_inspection_event(Some(InspectionEvent::ProjectionCommitted));
    assert_eq!(
        public.inspection.as_ref().unwrap().pending_event,
        Some(InspectionEvent::SnapshotChanged)
    );
    flush_observation(public);
    let after = snapshot(&mut client, &root);
    assert_eq!(after.sequence, before.sequence + 1);

    // Supply the publisher refusal at the private owner result boundary.
    // Runtime codec/refusal tests cover its source; no WM result is invented.
    let inspection = public.inspection.as_mut().unwrap();
    let signature = inspection.last.unwrap();
    assert!(!inspection.record_publication(
        signature,
        Some(InspectionEvent::ProjectionRejected),
        Err(InspectionError::Record(InspectionRecordError::Bounds)),
    ));
    for _ in 0..80 {
        public.note_inspection_event(Some(InspectionEvent::ProjectionRejected));
        public.publish_inspection();
    }
    assert_eq!(public.inspection.as_ref().unwrap().refusals, 1);
    assert!(public.inspection.as_ref().unwrap().pending_event.is_none());
    assert_eq!(snapshot(&mut client, &root), after);
    let mut changed = public.reducer.scene().clone();
    changed.generation += 1;
    public.reducer.observe_scene(changed).unwrap();
    flush_observation(public);
    assert!(public.inspection.as_ref().unwrap().refused.is_none());
    assert_eq!(snapshot(&mut client, &root).sequence, after.sequence + 1);
}

#[test]
fn a_fenced_publication_keeps_the_service_until_the_owner_replaces_its_view() {
    let mut fixture = ReloadFixture::new();
    install(&mut fixture);
    let public = fixture.wm.public.as_mut().unwrap();
    flush_observation(public);
    // Advance only the runtime fence to force the delayed producer path.
    public
        .inspection
        .as_mut()
        .unwrap()
        .service
        .fence(2, None)
        .unwrap();
    public.note_inspection_event(Some(InspectionEvent::ProjectionRejected));
    public.publish_inspection();
    let inspection = public.inspection.as_ref().unwrap();
    assert!(inspection.service.is_running());
    assert!(inspection.retry);
    assert_eq!(
        inspection.pending_event,
        Some(InspectionEvent::ProjectionRejected)
    );
    public.fence_inspection(2, None);
    // Connection replacement discards the preceding epoch's pending note.
    assert!(matches!(
        public.inspection.as_ref().unwrap().pending_event,
        None | Some(InspectionEvent::ConnectionChanged)
    ));
    flush_observation(public);
    let (mut client, root) = attach(public);
    assert_eq!(snapshot(&mut client, &root).snapshot.wm_epoch, 2);
}
