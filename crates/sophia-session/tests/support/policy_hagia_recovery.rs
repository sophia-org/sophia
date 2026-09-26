//! Real normal Hagia checkpoint restore through both Session restart owners.
//! Output IPC is not started here. Qid inspection observes allocator custody,
//! not held fids; retained admission, pixels and frontend ACKs are supplied.
use super::*;
use policy_transport_worker::ninep::qid_observation;

fn configured_after_restart(
    wm: &mut LiveWmSession,
    layout: &mut PersistentLiveLayout,
    config: &mut ConfigFixture,
    output: sophia_engine::HeadlessOutput,
) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        assert!(
            wm.poll_public_request(layout, output, false)
                .unwrap()
                .is_none()
        );
        wm.settle_desktop_reload(&mut config.config, true).unwrap();
        let public = wm.public.as_ref().unwrap();
        if public.configured && public.transport_ready {
            return;
        }
        assert!(!wm.force_transport_restart && !wm.degraded);
        assert!(
            Instant::now() < deadline,
            "replacement configuration deadline"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn replaced_checkpoint(path: &Path, old: (u64, u64)) -> (Vec<u8>, (u64, u64)) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let checkpoint = await_checkpoint(path);
        if checkpoint.1 != old {
            return checkpoint;
        }
        assert!(
            Instant::now() < deadline,
            "peer checkpoint replacement deadline"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn commit_reconciled(
    wm: &mut LiveWmSession,
    layout: &mut PersistentLiveLayout,
    output: sophia_engine::HeadlessOutput,
    proposal: LiveWmProposal,
    retained: &mut BTreeMap<SurfaceId, LayerSnapshot>,
    identity: &mut std::fs::File,
) {
    let epoch = proposal.policy_settlement.unwrap().connection_epoch;
    assert_eq!(proposal.layers.len(), retained.len());
    for layer in &proposal.layers {
        let old = &retained[&layer.surface];
        assert_eq!(layer.source, old.source);
        assert_eq!(layer.source_size, old.source_size);
        assert_eq!(layer.generation, old.generation);
        assert_eq!(layer.geometry.width, old.geometry.width);
        assert_eq!(layer.geometry.height, old.geometry.height);
        if let Some(translation) = layer.translation {
            assert_eq!(translation.connection_epoch, epoch);
        }
        writeln!(identity, "epoch={epoch} retained_geometry={:?} proposed_geometry={:?} translation={:?} source={:?} generation={}", old.geometry, layer.geometry, layer.translation, layer.source, layer.generation).unwrap();
    }
    let proposed = proposal
        .layers
        .iter()
        .cloned()
        .map(|layer| (layer.surface, layer))
        .collect::<BTreeMap<_, _>>();
    let mut controls = crate::session_control::SessionControlQueue::default();
    let result = layout
        .stage(proposal, &mut controls)
        .unwrap()
        .expect("restored geometry keeps the matching pixel extent");
    assert_eq!(result.update.commit.outcome, TransactionOutcome::Committed);
    let applied = wm.apply_commit_result(result, None, output.id).unwrap();
    assert!(applied.session_action.is_none());
    assert!(applied.physical_action.is_none());
    assert_eq!(layout.layers, proposed);
    for layer in layout.layers.values() {
        writeln!(
            identity,
            "epoch={epoch} committed_geometry={:?} committed_translation={:?}",
            layer.geometry, layer.translation
        )
        .unwrap();
    }
    *retained = layout.layers.clone();
    assert_managed_baseline(layout);
}

#[derive(Debug, PartialEq)]
struct RecoveryObservation {
    layers: Vec<LayerSnapshot>,
    checkpoint: Vec<u8>,
    dirty_generations: Vec<u64>,
}

fn assert_restored_layers(
    proposal: &LiveWmProposal,
    retained: &BTreeMap<SurfaceId, LayerSnapshot>,
) {
    let epoch = proposal.policy_settlement.unwrap().connection_epoch;
    let expected = retained
        .values()
        .cloned()
        .map(|mut layer| {
            // Compare observations modulo the explicitly epoch-scoped identity;
            // the actual proposal sent to stage is never changed.
            if let Some(translation) = layer.translation.as_mut() {
                translation.connection_epoch = epoch;
            }
            layer
        })
        .collect::<Vec<_>>();
    assert_eq!(proposal.layers, expected);
}

fn recovery(case: &str, transport: WmTransportSelection) -> RecoveryObservation {
    with_normal_hagia_transport(
        case,
        transport,
        |wm, layout, config, output, checkpoint, identity| {
            let evidence =
                PathBuf::from(std::env::var_os("SOPHIA_HAGIA_FILE_EVIDENCE").unwrap()).join(case);
            retain_existing_surface(layout);
            wm.enqueue_relayout(layout, output).unwrap();
            let proposal = next_proposal(wm, layout, output);
            std::fs::write(
                evidence.join("baseline-proposal.txt"),
                format!("{:#?}", proposal_observation(&proposal)),
            )
            .unwrap();
            std::fs::write(
                evidence.join("baseline-scene.txt"),
                format!("{:#?}", wm.public.as_ref().unwrap().reducer.scene()),
            )
            .unwrap();
            let mut controls = crate::session_control::SessionControlQueue::default();
            assert_eq!(
                wm.public
                    .as_ref()
                    .unwrap()
                    .reducer
                    .scene()
                    .outputs
                    .iter()
                    .find(|entry| entry.output == output.id)
                    .unwrap()
                    .focus,
                None
            );
            assert!(layout.stage(proposal, &mut controls).unwrap().is_none());
            acknowledge_frontend_controls(layout, &mut controls);
            assert!(!layout.pending_is_ready());
            matching_pixels(layout);
            assert!(layout.pending_is_ready());
            assert!(wm.prepare_public_layout_commit(layout).unwrap());
            let result = layout.resolve_pending().unwrap();
            assert_eq!(result.update.commit.outcome, TransactionOutcome::Committed);
            assert!(
                wm.apply_commit_result(result, None, output.id)
                    .unwrap()
                    .session_action
                    .is_none()
            );
            await_ready(wm, layout, output);
            let mut saved = await_checkpoint(checkpoint);
            let mut retained = layout.layers.clone();
            assert_managed_baseline(layout);
            // Establish the first focused projection before restart. Default
            // alwaysCenterSingleColumn runs only when a column has focus.
            wm.enqueue_relayout(layout, output).unwrap();
            let focused = next_proposal(wm, layout, output);
            assert_eq!(
                wm.public
                    .as_ref()
                    .unwrap()
                    .reducer
                    .scene()
                    .outputs
                    .iter()
                    .find(|entry| entry.output == output.id)
                    .unwrap()
                    .focus,
                Some(SURFACE)
            );
            let focused_layer = focused
                .layers
                .iter()
                .find(|entry| entry.surface == SURFACE)
                .unwrap();
            assert_eq!(retained[&SURFACE].geometry.x, 1);
            assert_eq!(retained[&SURFACE].translation.unwrap().x, 0);
            assert_eq!(focused_layer.geometry.x, 321);
            assert_eq!(focused_layer.translation.unwrap().x, 320);
            std::fs::write(
                evidence.join("focused-epoch-1.txt"),
                format!(
                    "scene={:#?}\nproposal={:#?}",
                    wm.public.as_ref().unwrap().reducer.scene(),
                    proposal_observation(&focused)
                ),
            )
            .unwrap();
            commit_reconciled(wm, layout, output, focused, &mut retained, identity);
            await_ready(wm, layout, output);
            saved = replaced_checkpoint(checkpoint, saved.1);
            let public = wm.public.as_ref().unwrap();
            let profile = public.profile_key;
            let selected = public.selected_capabilities;
            let catalog = public.actions.clone();
            let allocator = public.wm_filesystem_qids.clone();
            let mut watermark = qid_observation::watermark(&allocator);
            let original_spec = wm.supervisor.launch_spec().clone();
            let candidate_path = &original_spec
                .environment
                .iter()
                .find(|(key, _)| key == "HAGIA_POLICY_CANDIDATE")
                .unwrap()
                .1;
            let candidate = std::fs::read(candidate_path).unwrap();
            assert!(candidate.len() <= 1024 * 1024);
            std::fs::write(evidence.join("profile-candidate.txt"), candidate).unwrap();
            let mut previous_peer = wm.supervisor.peer_id().unwrap();
            let mut dirty_generations = Vec::new();

            for epoch in [2, 3] {
                std::fs::write(
                    evidence.join(format!("before-epoch-{epoch}.checkpoint")),
                    &saved.0,
                )
                .unwrap();
                assert!(layout.pending.is_none());
                if epoch == 2 {
                    // Invoke the actual automatic-recovery owner, without replacing
                    // its supervisor, worker or reducer in the fixture.
                    wm.force_transport_restart = true;
                    assert!(wm.poll_public_restart(layout, output).unwrap().is_none());
                } else {
                    assert_eq!(wm.begin_control_restart(output).unwrap(), epoch);
                    let deadline = Instant::now() + Duration::from_secs(5);
                    while wm.control_restart.is_some() {
                        wm.poll_control_restart(layout, output);
                        assert!(!wm.degraded);
                        assert!(Instant::now() < deadline, "control replacement deadline");
                        std::thread::sleep(Duration::from_millis(2));
                    }
                }
                assert_eq!(wm.public.as_ref().unwrap().connection_epoch, epoch);
                assert_eq!(layout.layers, retained);
                assert_eq!(await_checkpoint(checkpoint), saved);
                assert_eq!(wm.supervisor.launch_spec(), &original_spec);
                let protection = wm.supervisor.protection_evidence().unwrap();
                let peer = wm.supervisor.peer_id().unwrap();
                assert_eq!(protection.peer_pid, peer);
                assert_ne!(peer, previous_peer);
                assert!(
                    protection
                        .roles
                        .contains(&sophia_runtime::ProtectionDomainRole::SpatialPolicy)
                );
                writeln!(identity, "restart_epoch={epoch} previous_peer={previous_peer} peer={peer} protection={protection:?}").unwrap();
                previous_peer = peer;
                configured_after_restart(wm, layout, config, output);
                let public = wm.public.as_ref().unwrap();
                assert_eq!(public.profile_key, profile);
                assert_eq!(public.selected_capabilities, selected);
                assert_eq!(public.actions, catalog);
                assert!(public.output_service.is_none());
                assert_eq!(public.wm_transport, transport);
                assert!(qid_observation::same_owner(
                    &allocator,
                    &public.wm_filesystem_qids
                ));
                let next_watermark = qid_observation::watermark(&public.wm_filesystem_qids);
                if transport == WmTransportSelection::NineP2000L {
                    assert!(next_watermark > watermark);
                } else {
                    assert_eq!(next_watermark, watermark);
                }
                writeln!(identity, "epoch={epoch} qid_watermark_before={watermark} qid_watermark_after={next_watermark} same_allocator=true").unwrap();
                watermark = next_watermark;
                // Restart queued this request. No fixture enqueue after replacement.
                let restored = next_proposal(wm, layout, output);
                let restored_id = restored.policy_settlement.unwrap();
                assert_eq!(restored_id.connection_epoch, epoch);
                let restored_request = wm
                    .public
                    .as_ref()
                    .unwrap()
                    .in_flight_request
                    .clone()
                    .unwrap();
                std::fs::write(
                    evidence.join(format!("epoch-{epoch}-restored.txt")),
                    format!(
                        "scene={:#?}\nrequest={restored_request:#?}\nproposal={:#?}",
                        wm.public.as_ref().unwrap().reducer.scene(),
                        proposal_observation(&restored)
                    ),
                )
                .unwrap();
                assert_eq!(restored_request.cause, PolicyRequestCause::SceneChanged);
                assert_eq!(await_checkpoint(checkpoint), saved);
                assert_restored_layers(&restored, &retained);
                commit_reconciled(wm, layout, output, restored, &mut retained, identity);
                saved = replaced_checkpoint(checkpoint, saved.1);

                // A loaded Hagia checkpoint triggers a real Dirty only after this
                // first restored commit. Let the canonical Session owner admit it
                // and issue the continuation, without synthesizing either event.
                let refreshed = next_proposal(wm, layout, output);
                let refreshed_id = refreshed.policy_settlement.unwrap();
                assert_restored_layers(&refreshed, &retained);
                let request = wm
                    .public
                    .as_ref()
                    .unwrap()
                    .in_flight_request
                    .clone()
                    .unwrap();
                assert_eq!(request.cause, PolicyRequestCause::SceneChanged);
                assert!(request.policy_generation > restored_request.policy_generation);
                assert!(request.request_id > restored_request.request_id);
                assert_eq!(request.connection_epoch, epoch);
                assert_eq!(wm.supervisor.peer_id(), Some(peer));
                let delta = refreshed_id.transaction.raw() - restored_id.transaction.raw();
                // Legacy Dirty consumes an envelope domain transaction. File Dirty
                // has only its distinct submission identity, no invented domain ID.
                assert_eq!(
                    delta,
                    match transport {
                        WmTransportSelection::CurrentIpc => 2,
                        WmTransportSelection::NineP2000L => 1,
                    }
                );
                writeln!(identity, "epoch={epoch} restored_request={} dirty_request={} policy_generation={} restored_domain={} continuation_domain={} dirty_domain_delta={delta} fixture_enqueued=false", restored_request.request_id, request.request_id, request.policy_generation, restored_id.transaction.raw(), refreshed_id.transaction.raw()).unwrap();
                dirty_generations.push(request.policy_generation);
                commit_reconciled(wm, layout, output, refreshed, &mut retained, identity);
                await_ready(wm, layout, output);
                saved = replaced_checkpoint(checkpoint, saved.1);
            }
            writeln!(identity, "checkpoint_restore=real_hagia\nautomatic_and_control_restart=true\noutput_service=none\nqid_evidence=allocator_identity_and_watermark_not_held_fids\nstale_wire_handles=not_exercised\nprofile_replacement=not_exercised\nnative_receipt=false\nexecutor_called=false").unwrap();
            RecoveryObservation {
                layers: retained.into_values().collect(),
                checkpoint: saved.0,
                dirty_generations,
            }
        },
    )
}

#[test]
#[ignore = "requires exact frozen normal Hagia and explicit fresh evidence inputs"]
fn normal_hagia_checkpoint_restore_survives_automatic_and_control_restart_on_both_wires() {
    let ipc = recovery("recovery-ipc", WmTransportSelection::CurrentIpc);
    let files = recovery("recovery-files", WmTransportSelection::NineP2000L);
    assert_eq!(
        ipc, files,
        "restored semantic state agrees, domain transaction allocation differs"
    );
}
