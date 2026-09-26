//! Real Hagia proposals through Session layout readiness and settlement.
//! Initial retained CPU facts and subsequent client pixels are supplied; this
//! does not execute Engine surface prepare/apply or native frame retirement.
use super::*;
use sophia_protocol::*;
use std::os::unix::fs::MetadataExt;

const SURFACE: SurfaceId = SurfaceId::new(3, 1);

fn retain_existing_surface(layout: &mut PersistentLiveLayout) {
    let geometry = Rect {
        x: 30,
        y: 30,
        width: 160,
        height: 120,
    };
    let size = Size {
        width: geometry.width,
        height: geometry.height,
    };
    let mut batch = crate::live_session::wm_update_coordinator_batch(TransactionId::from_raw(700));
    let client = sophia_x_authority::XServerFrontendClientId::from_raw(1);
    batch.client = Some(client);
    batch
        .surface_routes
        .push(sophia_x_authority::XAuthoritySurfaceRouteObservation {
            surface: SURFACE,
            client,
            admission: None,
        });
    batch.surface_presentations.push(
        sophia_x_authority::XAuthoritySurfacePresentationObservation {
            surface: SURFACE,
            role: SurfacePresentationRole::PolicyManaged,
            kind: LayoutNodeKind::Toplevel,
            placement_preference: SurfacePlacementPreference::Default,
            owner: None,
            stack_rank: 0,
            mapped: true,
            geometry,
            constraints: SurfaceConstraints {
                min_size: None,
                max_size: None,
            },
            generation: 8,
        },
    );
    let observed = layout.observe_authority_batch(&batch);
    assert!(!observed.client_route_invalid);
    // This is the fixture's already retained application image, not a WM
    // proposal or a fabricated settlement. Its safe size makes resize wait.
    layout.layers.insert(
        SURFACE,
        LayerSnapshot {
            surface: SURFACE,
            authority_local_id: None,
            namespace: None,
            stack_rank: 0,
            geometry,
            source: BufferSource::CpuBuffer { handle: 700 },
            source_size: size,
            damage: Region::empty(),
            opacity: 1.0,
            crop: None,
            transform: Transform::IDENTITY,
            generation: 1,
            resize_sync: ResizeSyncCapability::ImplicitOnly,
            output: None,
            input_region: None,
            translation: None,
        },
    );
    layout.cpu_buffer_sizes.insert(700, size);
    layout.layout_epochs.record_committed(SURFACE, size);
    // Supplied historical admission accompanies the retained initial image.
    // This setup does not demonstrate its original native retirement.
    assert!(layout.admissions.observe_intent(SurfacePresentationIntent {
        surface: SURFACE,
        kind: SurfacePresentationIntentKind::Request,
        role: SurfacePresentationRole::PolicyManaged,
        surface_kind: LayoutNodeKind::Toplevel,
        placement_preference: SurfacePlacementPreference::Default,
        presentation_owner: None,
        stack_rank: 0,
        geometry,
        constraints: SurfaceConstraints {
            min_size: None,
            max_size: None
        },
        generation: 8,
    }));
    assert!(
        layout
            .admissions
            .begin_control(SURFACE, TransactionId::from_raw(700), geometry)
    );
    assert!(
        layout
            .admissions
            .acknowledge_control(SURFACE, TransactionId::from_raw(700))
    );
    let historical_candidate = SurfaceTransactionKey {
        transaction: TransactionId::from_raw(700),
        surface: SURFACE,
        target_buffer: BufferSource::CpuBuffer { handle: 700 },
    };
    assert!(
        layout
            .admissions
            .begin_retirement(SURFACE, historical_candidate)
    );
    // Explicitly supplied historical completion, not a measured native event.
    // Use the Session owner so every mirrored admission table agrees.
    assert!(layout.complete_admission_retirement(historical_candidate));
    assert_managed_baseline(layout);
}

fn assert_managed_baseline(layout: &PersistentLiveLayout) {
    assert_eq!(
        layout.admissions.state(SURFACE),
        sophia_engine::SurfacePresentationAdmissionState::Managed
    );
    assert!(!layout.unmanaged_surfaces.contains(&SURFACE));
    assert!(!layout.planning_surfaces.contains_key(&SURFACE));
    assert_eq!(
        layout.layout_epochs.admission(SURFACE),
        sophia_engine::SurfaceAdmissionState::Managed
    );
    assert!(layout.layout_epochs.pending_target(SURFACE).is_none());
    assert!(layout.layout_epochs.recovery_extent(SURFACE).is_none());
}

fn assert_managed_rollback(
    controls: &mut crate::session_control::SessionControlQueue,
    retained: &LayerSnapshot,
) {
    let (sender, receiver) = std::sync::mpsc::sync_channel(32);
    let (_ack_sender, ack_receiver) = std::sync::mpsc::sync_channel(32);
    let mut completions = Vec::new();
    controls
        .service(&sender, &ack_receiver, Instant::now(), &mut completions)
        .unwrap();
    assert!(completions.is_empty());
    let commands = receiver.try_iter().collect::<Vec<_>>();
    assert_eq!(
        commands.len(),
        1,
        "managed expiry emits exactly one rollback configure"
    );
    assert_eq!(
        commands[0].client,
        sophia_x_authority::XServerFrontendClientId::from_raw(1)
    );
    let sophia_x_authority::XAuthorityControlCommand::ConfigureSurface {
        surface, geometry, ..
    } = commands[0].command
    else {
        panic!("managed expiry must configure the retained extent");
    };
    assert_eq!(surface, SURFACE);
    assert_eq!(geometry, retained.geometry);
}

fn next_proposal(
    wm: &mut LiveWmSession,
    layout: &mut PersistentLiveLayout,
    output: sophia_engine::HeadlessOutput,
) -> LiveWmProposal {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(proposal) = wm.poll_public_request(layout, output, true).unwrap() {
            return proposal;
        }
        assert!(
            !wm.force_transport_restart && !wm.degraded,
            "same Hagia must remain admitted"
        );
        assert!(Instant::now() < deadline, "real Hagia proposal deadline");
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn matching_pixels(layout: &mut PersistentLiveLayout) {
    let pending = layout.pending.as_ref().unwrap();
    assert_eq!(pending.requested_sizes.len(), 1);
    let size = pending.requested_sizes[&SURFACE];
    let geometry = pending
        .layers
        .iter()
        .find(|layer| layer.surface == SURFACE)
        .unwrap()
        .geometry;
    let previous = layout.layers[&SURFACE].generation;
    layout.cpu_buffer_sizes.insert(701, size);
    let mut batch = crate::live_session::wm_update_coordinator_batch(TransactionId::from_raw(701));
    batch.client = Some(sophia_x_authority::XServerFrontendClientId::from_raw(1));
    batch.transactions.push(SurfaceTransaction {
        transaction: TransactionId::from_raw(701),
        authority: AuthorityKind::SophiaX,
        surface: SURFACE,
        namespace: None,
        target_geometry: geometry,
        presentation_extent: size,
        content: SurfaceContentSet::singleton(BufferSource::CpuBuffer { handle: 701 }, size),
        damage: Region::single(geometry),
        readiness: SurfaceTransactionReadiness::Ready,
        timeout_msec: 250,
        previous_committed_generation: previous,
        input_region: None,
    });
    assert!(!layout.observe_authority_batch(&batch).client_route_invalid);
}

fn acknowledge_frontend_controls(
    layout: &mut PersistentLiveLayout,
    controls: &mut crate::session_control::SessionControlQueue,
) {
    use sophia_x_authority::{
        XAuthorityClientControlAck, XAuthorityControlAck, XAuthorityControlKind,
        XAuthorityControlOutcome,
    };
    let (sender, receiver) = std::sync::mpsc::sync_channel(32);
    let (ack_sender, ack_receiver) = std::sync::mpsc::sync_channel(32);
    let mut completions = Vec::new();
    controls
        .service(&sender, &ack_receiver, Instant::now(), &mut completions)
        .unwrap();
    assert!(completions.is_empty());
    let commands = receiver.try_iter().collect::<Vec<_>>();
    assert!(!commands.is_empty());
    for command in &commands {
        ack_sender
            .send(XAuthorityClientControlAck {
                client: command.client,
                acknowledgement: XAuthorityControlAck {
                    kind: command.command.kind(),
                    transaction: command.command.transaction(),
                    surface: command.command.surface(),
                    outcome: XAuthorityControlOutcome::Delivered,
                },
            })
            .unwrap();
    }
    controls
        .service(&sender, &ack_receiver, Instant::now(), &mut completions)
        .unwrap();
    assert_eq!(completions.len(), commands.len());
    // Supplied frontend ACKs traverse the real queue correlation owner, then
    // the same layout acknowledgement method used by the production loop.
    // This is not a WM outcome or a native presentation receipt.
    for completion in completions {
        assert!(completion.failure.is_none());
        if completion.key.kind == XAuthorityControlKind::SetPresentationState {
            assert!(layout.acknowledge_presentation_control(
                completion.key.transaction,
                completion.key.surface
            ));
        }
    }
}

fn await_ready(
    wm: &mut LiveWmSession,
    layout: &mut PersistentLiveLayout,
    output: sophia_engine::HeadlessOutput,
) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        assert!(
            wm.poll_public_request(layout, output, false)
                .unwrap()
                .is_none()
        );
        let public = wm.public.as_ref().unwrap();
        assert!(
            public.pending_operation.is_none(),
            "failed action must not issue an operation"
        );
        if public.transport_ready && public.in_flight_request.is_none() {
            return;
        }
        assert!(!wm.force_transport_restart && !wm.degraded);
        assert!(
            Instant::now() < deadline,
            "same child outcome/Ready deadline"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn await_checkpoint(path: &Path) -> (Vec<u8>, (u64, u64)) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match std::fs::File::open(path) {
            Ok(mut file) => {
                use std::io::Read;
                let metadata = file.metadata().unwrap();
                assert!(metadata.len() <= 1024 * 1024, "bounded fixture checkpoint");
                let mut bytes = Vec::new();
                file.read_to_end(&mut bytes).unwrap();
                assert!(
                    !bytes.is_empty(),
                    "atomically published checkpoint is complete"
                );
                return (bytes, (metadata.dev(), metadata.ino()));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                assert!(
                    Instant::now() < deadline,
                    "Hagia checkpoint consumption deadline: {error}"
                );
                std::thread::sleep(Duration::from_millis(2));
            }
            Err(error) => panic!("Hagia checkpoint read failed: {error}"),
        }
    }
}

#[test]
#[ignore = "requires exact frozen normal Hagia and explicit fresh evidence inputs"]
fn normal_hagia_held_resize_commits_then_answers_a_fresh_request() {
    with_normal_hagia(
        "held-resize-commit",
        |wm, layout, _, output, checkpoint, identity| {
            let peer = wm.supervisor.peer_id();
            retain_existing_surface(layout);
            let old_layer = layout.layers[&SURFACE].clone();
            let old_projection = wm.public.as_ref().unwrap().reducer.committed();
            wm.enqueue_relayout(layout, output).unwrap();
            let proposal = next_proposal(wm, layout, output);
            let settlement = proposal.policy_settlement.unwrap();
            assert_eq!(settlement.connection_epoch, 1);
            assert!(!settlement.expect_session_operation);
            assert!(
                !proposal.requested_sizes.is_empty(),
                "resize must come from Hagia"
            );
            assert_ne!(proposal.requested_sizes[&SURFACE], old_layer.source_size);
            let mut controls = crate::session_control::SessionControlQueue::default();
            assert!(layout.stage(proposal, &mut controls).unwrap().is_none());
            assert!(!layout.pending.as_ref().unwrap().requested_sizes.is_empty());
            assert!(!layout.pending_is_ready());
            assert_eq!(layout.layers[&SURFACE], old_layer);
            assert_eq!(
                wm.public.as_ref().unwrap().reducer.committed(),
                old_projection
            );
            assert!(!checkpoint.exists());
            acknowledge_frontend_controls(layout, &mut controls);
            assert!(!layout.pending_is_ready());
            assert_eq!(layout.layers[&SURFACE], old_layer);
            matching_pixels(layout);
            assert!(
                !layout
                    .pending
                    .as_ref()
                    .unwrap()
                    .staged_transactions
                    .is_empty()
            );
            assert!(layout.pending_is_ready());
            assert!(wm.prepare_public_layout_commit(layout).unwrap());
            let result = layout
                .resolve_pending()
                .expect("matching pixels resolve real pending layout");
            assert_eq!(result.update.commit.outcome, TransactionOutcome::Committed);
            let applied = wm.apply_commit_result(result, None, output.id).unwrap();
            assert!(applied.session_action.is_none());
            assert!(applied.physical_action.is_none());
            assert_ne!(
                wm.public.as_ref().unwrap().reducer.committed(),
                old_projection
            );
            wm.enqueue_relayout(layout, output).unwrap();
            let next = next_proposal(wm, layout, output);
            let next_identity = next.policy_settlement.unwrap();
            assert!(next_identity.request_id > settlement.request_id);
            assert!(next_identity.transaction.raw() > settlement.transaction.raw());
            assert_eq!(wm.supervisor.peer_id(), peer);
            assert_eq!(next_identity.connection_epoch, 1);
            assert!(
                checkpoint.exists(),
                "real Hagia committed outcome persists its checkpoint before next answer"
            );
            writeln!(identity, "layout_outcome=committed\nfirst_request={}\nnext_request={}\nsame_child=true\npixels=supplied CPU facts; no native retirement", settlement.request_id, next_identity.request_id).unwrap();
        },
    );
}

#[test]
#[ignore = "requires exact frozen normal Hagia and explicit fresh evidence inputs"]
fn normal_hagia_timed_out_session_action_keeps_checkpoint_and_answers_next_request() {
    with_normal_hagia(
        "held-resize-timeout",
        |wm, layout, _, output, checkpoint, identity| {
            let peer = wm.supervisor.peer_id();
            retain_existing_surface(layout);
            wm.enqueue_relayout(layout, output).unwrap();
            let initial = next_proposal(wm, layout, output);
            assert!(!initial.requested_sizes.is_empty());
            let mut controls = crate::session_control::SessionControlQueue::default();
            assert!(layout.stage(initial, &mut controls).unwrap().is_none());
            acknowledge_frontend_controls(layout, &mut controls);
            assert!(!layout.pending_is_ready());
            matching_pixels(layout);
            assert!(
                !layout
                    .pending
                    .as_ref()
                    .unwrap()
                    .staged_transactions
                    .is_empty()
            );
            assert!(layout.pending_is_ready());
            assert!(wm.prepare_public_layout_commit(layout).unwrap());
            let result = layout
                .resolve_pending()
                .expect("initial actual held resize ready");
            assert_eq!(result.update.commit.outcome, TransactionOutcome::Committed);
            wm.apply_commit_result(result, None, output.id).unwrap();
            await_ready(wm, layout, output);
            // Ready acknowledges Session publication, not peer consumption. Wait
            // for Hagia's fresh atomic checkpoint before fixing the baseline.
            let (checkpoint_before, checkpoint_identity) = await_checkpoint(checkpoint);
            let layer_before = layout.layers[&SURFACE].clone();
            let projection_before = wm.public.as_ref().unwrap().reducer.committed();
            assert_managed_baseline(layout);
            assert!(
                wm.public
                    .as_ref()
                    .unwrap()
                    .session_operations
                    .iter()
                    .any(|operation| operation.slot == 1)
            );
            let action = wm
                .public
                .as_ref()
                .unwrap()
                .actions
                .iter()
                .find(|entry| entry.session_operation_slot == Some(1))
                .expect("real profile advertises slot one")
                .action;
            // Queue the action first, then change canonical work-area facts before
            // sending it. The owner queues its own relayout behind the action;
            // neither request ordering nor any returned proposal is rewritten.
            wm.enqueue_action(action, layout, output).unwrap();
            let bounds = wm_output_bounds(&[output])[0].1;
            assert!(wm.set_shell_reservation_bands(vec![OutputReservation {
                edge: OutputEdge::Top,
                depth: 80,
                span: AxisSpan {
                    start: bounds.x,
                    end: bounds.x + bounds.width
                },
            }]));
            wm.update_output_work_areas(layout, &[output], output)
                .unwrap();
            let proposal = next_proposal(wm, layout, output);
            assert_eq!(proposal.source, Some(LiveWmProposalSource::Action(action)));
            let settlement = proposal.policy_settlement.unwrap();
            assert!(
                settlement.expect_session_operation,
                "must exercise P1 operation-slot boundary"
            );
            assert!(
                !proposal.requested_sizes.is_empty(),
                "Hagia must compute the changed target"
            );
            assert_ne!(proposal.requested_sizes[&SURFACE], layer_before.source_size);
            assert!(layout.stage(proposal, &mut controls).unwrap().is_none());
            assert!(!layout.pending.as_ref().unwrap().requested_sizes.is_empty());
            acknowledge_frontend_controls(layout, &mut controls);
            assert!(!layout.pending_is_ready());
            assert_eq!(layout.layers[&SURFACE], layer_before);
            assert_eq!(
                wm.public.as_ref().unwrap().reducer.committed(),
                projection_before
            );
            assert!(wm.public.as_ref().unwrap().prepared.is_none());
            assert!(layout.force_pending_timeout());
            let result = layout
                .expire_pending(&mut controls)
                .unwrap()
                .expect("actual pending expiry");
            assert_eq!(result.update.commit.outcome, TransactionOutcome::TimedOut);
            assert_managed_baseline(layout);
            assert!(layout.layout_epochs.rollback_pending(SURFACE));
            assert_managed_rollback(&mut controls, &layer_before);
            let applied = wm.apply_commit_result(result, None, output.id).unwrap();
            assert!(applied.session_action.is_none());
            assert!(applied.physical_action.is_none());
            assert_eq!(layout.layers[&SURFACE], layer_before);
            assert_eq!(
                wm.public.as_ref().unwrap().reducer.committed(),
                projection_before
            );
            await_ready(wm, layout, output);
            assert_eq!(std::fs::read(checkpoint).unwrap(), checkpoint_before);
            // Existing timeout rearm or the work-area owner's queued cause proves
            // continuation; the source of this next request is not asserted.
            let next = next_proposal(wm, layout, output);
            let next_identity = next.policy_settlement.unwrap();
            assert!(next_identity.request_id > settlement.request_id);
            assert!(next_identity.transaction.raw() > settlement.transaction.raw());
            assert_eq!(next_identity.connection_epoch, 1);
            assert_eq!(wm.supervisor.peer_id(), peer);
            assert!(wm.public.as_ref().unwrap().pending_operation.is_none());
            assert_eq!(std::fs::read(checkpoint).unwrap(), checkpoint_before);
            let after = std::fs::metadata(checkpoint).unwrap();
            assert_eq!((after.dev(), after.ino()), checkpoint_identity);
            writeln!(identity, "layout_outcome=timed_out\ndeadline=forced_now\nfailed_action={}\nfailed_request={}\nnext_request={}\nsame_child=true\ncheckpoint_unchanged=true\nno_session_operation=true", action.raw(), settlement.request_id, next_identity.request_id).unwrap();
        },
    );
}
