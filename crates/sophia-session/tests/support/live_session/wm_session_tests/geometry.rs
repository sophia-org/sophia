fn register_test_routes(layout: &mut PersistentLiveLayout, surfaces: &[SurfaceId]) {
    let mut batch =
        crate::live_session::wm_update_coordinator_batch(TransactionId::from_raw(1));
    batch.client = Some(sophia_x_authority::XServerFrontendClientId::from_raw(1));
    for surface in surfaces {
        batch.surface_routes.push(
            sophia_x_authority::XAuthoritySurfaceRouteObservation {
                surface: *surface,
                client: sophia_x_authority::XServerFrontendClientId::from_raw(1),
                admission: None,
            },
        );
        batch
            .presentation_intents
            .push(sophia_protocol::SurfacePresentationIntent {
                surface: *surface,
                kind: sophia_protocol::SurfacePresentationIntentKind::Request,
                role: sophia_protocol::SurfacePresentationRole::PolicyManaged,
                surface_kind: sophia_protocol::LayoutNodeKind::Toplevel,
                placement_preference: sophia_protocol::SurfacePlacementPreference::Default,
                presentation_owner: None,
                stack_rank: 0,
                geometry: Rect::default(),
                constraints: SurfaceConstraints {
                    min_size: None,
                    max_size: None,
                },
                generation: 1,
            });
    }
    layout.client_routes.observe(&batch).unwrap();
}

fn drain_test_controls(
    controls: &mut crate::session_control::SessionControlQueue,
) -> Vec<sophia_x_authority::XAuthorityClientControlCommand> {
    let (sender, receiver) = std::sync::mpsc::sync_channel(8);
    let (_acknowledgements, acknowledgement_receiver) = std::sync::mpsc::sync_channel(8);
    controls
        .service(
            &sender,
            &acknowledgement_receiver,
            Instant::now(),
            &mut Vec::new(),
        )
        .unwrap();
    receiver.try_iter().collect()
}

#[test]
fn move_only_surface_receives_geometry_control_without_becoming_a_resize_obligation() {
    let firefox = SurfaceId::new(1, 1);
    let terminal_a = SurfaceId::new(2, 1);
    let terminal_b = SurfaceId::new(3, 1);
    let surfaces = [firefox, terminal_a, terminal_b];
    let old = [
        Rect {
            x: 647,
            y: 21,
            width: 1276,
            height: 1422,
        },
        Rect {
            x: 1927,
            y: 21,
            width: 636,
            height: 1422,
        },
        Rect {
            x: 7,
            y: 21,
            width: 636,
            height: 1422,
        },
    ];
    let next = [
        Rect { x: 7, ..old[0] },
        Rect {
            x: 1282,
            y: 16,
            width: 1276,
            height: 709,
        },
        Rect {
            x: 1282,
            y: 729,
            width: 1276,
            height: 709,
        },
    ];
    let mut layout = PersistentLiveLayout::default();
    register_test_routes(&mut layout, &surfaces);
    for (index, surface) in surfaces.iter().copied().enumerate() {
        layout.layers.insert(surface, test_layer(surface, old[index]));
        layout.layout_epochs.record_committed(
            surface,
            Size {
                width: old[index].width,
                height: old[index].height,
            },
        );
    }
    let transaction = TransactionId::from_raw(9);
    let proposal = LiveWmProposal {
        transaction,
        layers: surfaces
            .iter()
            .enumerate()
            .map(|(index, surface)| test_layer(*surface, next[index]))
            .collect(),
        requested_sizes: BTreeMap::from([
            (
                terminal_a,
                Size {
                    width: next[1].width,
                    height: next[1].height,
                },
            ),
            (
                terminal_b,
                Size {
                    width: next[2].width,
                    height: next[2].height,
                },
            ),
        ]),
        presentation_states: BTreeMap::new(),
        configure_deliveries: 0,
        focus: Some(firefox),
        timeout: Duration::from_secs(1),
        update: sophia_engine::WmTransactionUpdate {
            commit: TransactionCommit {
                transaction,
                outcome: TransactionOutcome::Committed,
                applied_surfaces: surfaces.to_vec(),
            },
        },
        moved_surfaces: 0,
        source: Some(LiveWmProposalSource::Action(WmActionId::from_raw(3))),
        policy_settlement: None,
    };
    let mut controls = crate::session_control::SessionControlQueue::default();

    assert!(layout.stage(proposal, &mut controls).unwrap().is_none());
    let pending = layout.pending.as_ref().unwrap();
    assert_eq!(pending.requested_sizes.len(), 2);
    assert!(!pending.requested_sizes.contains_key(&firefox));
    assert_eq!(pending.moved_surfaces, 3);
    assert_eq!(pending.configure_deliveries, 3);
    let commands = drain_test_controls(&mut controls);
    assert_eq!(commands.len(), 3);
    for (index, surface) in surfaces.iter().copied().enumerate() {
        assert!(commands.contains(&sophia_x_authority::XAuthorityClientControlCommand {
            client: sophia_x_authority::XServerFrontendClientId::from_raw(1),
            command: sophia_x_authority::XAuthorityControlCommand::ConfigureSurface {
                transaction,
                surface,
                geometry: next[index],
            },
        }));
    }
}

#[test]
fn focus_only_layout_emits_no_geometry_control() {
    let surface = SurfaceId::new(4, 1);
    let geometry = Rect {
        x: 7,
        y: 21,
        width: 1276,
        height: 1422,
    };
    let transaction = TransactionId::from_raw(10);
    let mut layout = PersistentLiveLayout::default();
    register_test_routes(&mut layout, &[surface]);
    layout.layers.insert(surface, test_layer(surface, geometry));
    let proposal = LiveWmProposal {
        transaction,
        layers: vec![test_layer(surface, geometry)],
        requested_sizes: BTreeMap::new(),
        presentation_states: BTreeMap::new(),
        configure_deliveries: 0,
        focus: Some(surface),
        timeout: Duration::from_secs(1),
        update: sophia_engine::WmTransactionUpdate {
            commit: TransactionCommit {
                transaction,
                outcome: TransactionOutcome::Committed,
                applied_surfaces: vec![surface],
            },
        },
        moved_surfaces: 1,
        source: Some(LiveWmProposalSource::Focus(surface)),
        policy_settlement: None,
    };
    let mut controls = crate::session_control::SessionControlQueue::default();

    assert!(layout.stage(proposal, &mut controls).unwrap().is_some());
    assert_eq!(controls.pending_len(), 0);
}

#[test]
fn recovery_reseed_reasserts_geometry_when_only_committed_pixels_are_stale() {
    let surface = SurfaceId::new(5, 1);
    let geometry = Rect {
        x: 7,
        y: 21,
        width: 1276,
        height: 1422,
    };
    let transaction = TransactionId::from_raw(11);
    let mut layout = PersistentLiveLayout::default();
    register_test_routes(&mut layout, &[surface]);
    layout.layers.insert(surface, test_layer(surface, geometry));
    layout.layout_epochs.record_committed(
        surface,
        Size {
            width: 636,
            height: 1422,
        },
    );
    let requested_size = Size {
        width: geometry.width,
        height: geometry.height,
    };
    let proposal = LiveWmProposal {
        transaction,
        layers: vec![test_layer(surface, geometry)],
        requested_sizes: BTreeMap::from([(surface, requested_size)]),
        presentation_states: BTreeMap::new(),
        configure_deliveries: 0,
        focus: Some(surface),
        timeout: Duration::from_secs(1),
        update: sophia_engine::WmTransactionUpdate {
            commit: TransactionCommit {
                transaction,
                outcome: TransactionOutcome::Committed,
                applied_surfaces: vec![surface],
            },
        },
        moved_surfaces: 0,
        source: Some(LiveWmProposalSource::Relayout),
        policy_settlement: None,
    };
    let mut controls = crate::session_control::SessionControlQueue::default();

    assert!(layout.stage(proposal, &mut controls).unwrap().is_none());
    let pending = layout.pending.as_ref().unwrap();
    assert_eq!(pending.requested_sizes, BTreeMap::from([(surface, requested_size)]));
    assert_eq!(pending.moved_surfaces, 0);
    assert_eq!(pending.configure_deliveries, 1);
    let commands = drain_test_controls(&mut controls);
    assert_eq!(commands.len(), 1);
    assert_eq!(
        commands[0].command,
        sophia_x_authority::XAuthorityControlCommand::ConfigureSurface {
            transaction,
            surface,
            geometry,
        }
    );
}

#[test]
fn duplicate_target_awaiting_retirement_does_not_send_a_second_configure() {
    let surface = SurfaceId::new(6, 1);
    let old = Size {
        width: 1920,
        height: 1080,
    };
    let target = Size {
        width: 1276,
        height: 1436,
    };
    let geometry = Rect {
        x: 2,
        y: 2,
        width: target.width,
        height: target.height,
    };
    let visual_transaction = TransactionId::from_raw(545);
    let visual_buffer = BufferHandle::from_raw(9);
    let candidate = dma_candidate(visual_transaction, surface, visual_buffer);
    let mut layout = PersistentLiveLayout::default();
    register_test_routes(&mut layout, &[surface]);
    layout.layers.insert(surface, test_layer(surface, geometry));
    layout.layout_epochs.record_committed(surface, old);
    layout
        .awaiting_visual_commits
        .arm(ResizeVisualCommit {
            candidate,
            size: target,
            layout_size: target,
        })
        .unwrap();
    let transaction = TransactionId::from_raw(11);
    let proposal = LiveWmProposal {
        transaction,
        layers: vec![test_layer(surface, geometry)],
        requested_sizes: BTreeMap::from([(surface, target)]),
        presentation_states: BTreeMap::new(),
        configure_deliveries: 0,
        focus: Some(surface),
        timeout: Duration::from_secs(1),
        update: sophia_engine::WmTransactionUpdate {
            commit: TransactionCommit {
                transaction,
                outcome: TransactionOutcome::Committed,
                applied_surfaces: vec![surface],
            },
        },
        moved_surfaces: 0,
        source: Some(LiveWmProposalSource::Relayout),
        policy_settlement: None,
    };
    let mut controls = crate::session_control::SessionControlQueue::default();

    assert!(layout.stage(proposal, &mut controls).unwrap().is_some());
    assert!(layout.pending.is_none());
    assert_eq!(controls.pending_len(), 0);
    assert!(layout.awaiting_visual_commits.exact_candidate(candidate, target));
    assert_eq!(layout.layout_epochs.committed_size(surface), Some(old));
}

#[test]
fn standing_visual_target_still_configures_geometry_that_is_not_installed() {
    let surface = SurfaceId::new(7, 1);
    let old = Size {
        width: 1920,
        height: 1080,
    };
    let old_geometry = Rect {
        x: 2,
        y: 2,
        width: old.width,
        height: old.height,
    };
    let target = Size {
        width: 1276,
        height: 1436,
    };
    let target_geometry = Rect {
        width: target.width,
        height: target.height,
        ..old_geometry
    };
    let candidate = dma_candidate(
        TransactionId::from_raw(546),
        surface,
        BufferHandle::from_raw(10),
    );
    let mut layout = PersistentLiveLayout::default();
    register_test_routes(&mut layout, &[surface]);
    layout
        .layers
        .insert(surface, test_layer(surface, old_geometry));
    layout.layout_epochs.record_committed(surface, old);
    layout
        .awaiting_visual_commits
        .arm(ResizeVisualCommit {
            candidate,
            size: target,
            layout_size: target,
        })
        .unwrap();
    let transaction = TransactionId::from_raw(12);
    let proposal = LiveWmProposal {
        transaction,
        layers: vec![test_layer(surface, target_geometry)],
        requested_sizes: BTreeMap::from([(surface, target)]),
        presentation_states: BTreeMap::new(),
        configure_deliveries: 0,
        focus: Some(surface),
        timeout: Duration::from_secs(1),
        update: sophia_engine::WmTransactionUpdate {
            commit: TransactionCommit {
                transaction,
                outcome: TransactionOutcome::Committed,
                applied_surfaces: vec![surface],
            },
        },
        moved_surfaces: 0,
        source: Some(LiveWmProposalSource::Relayout),
        policy_settlement: None,
    };
    let mut controls = crate::session_control::SessionControlQueue::default();

    assert!(layout.stage(proposal, &mut controls).unwrap().is_none());
    assert_eq!(
        layout.pending.as_ref().unwrap().requested_sizes,
        BTreeMap::from([(surface, target)])
    );
    assert_eq!(controls.pending_len(), 1);
}

#[test]
fn a_surface_withdrawn_mid_cycle_is_skipped_rather_than_fatal() {
    // A window can close while a layout is in flight. The rollback that undoes
    // the proposal then names a surface with no committed geometry and no
    // client route, and there is neither anything to restore it to nor anyone
    // left to tell.
    //
    // This ended the session until now: one `ok_or(...)?` turned an ordinary
    // race into `live WM rollback has no committed geometry`, and a real
    // desktop died of it when a workspace switch happened to coincide with a
    // dock going away. Killing a desktop because a window stopped existing is
    // never the right answer.
    let surface = SurfaceId::new(6, 1);
    let rejected = Rect {
        x: 7,
        y: 21,
        width: 1000,
        height: 700,
    };
    let transaction = TransactionId::from_raw(12);
    let mut layout = PersistentLiveLayout::default();
    register_test_routes(&mut layout, &[surface]);
    // Deliberately no `layers.insert`: the surface was withdrawn between the
    // proposal and its expiry.
    layout.pending = Some(PendingLiveWmLayout {
        transaction,
        layers: vec![test_layer(surface, rejected)],
        requested_sizes: BTreeMap::from([(
            surface,
            Size {
                width: rejected.width,
                height: rejected.height,
            },
        )]),
        presentation_states: BTreeMap::new(),
        presentation_settlements: BTreeSet::new(),
        configure_deliveries: 1,
        focus: Some(surface),
        deadline: Instant::now(),
        update: sophia_engine::WmTransactionUpdate {
            commit: TransactionCommit {
                transaction,
                outcome: TransactionOutcome::Committed,
                applied_surfaces: vec![surface],
            },
        },
        moved_surfaces: 1,
        staged_transactions: BTreeMap::new(),
        admission_surfaces: BTreeSet::new(),
        source: Some(LiveWmProposalSource::Action(WmActionId::from_raw(3))),
        policy_settlement: None,
    });
    let mut controls = crate::session_control::SessionControlQueue::default();

    let result = layout
        .expire_pending(&mut controls)
        .expect("a withdrawn surface must not end the session")
        .expect("the cycle still settles");

    assert_eq!(result.update.commit.outcome, TransactionOutcome::TimedOut);
    // Nothing is configured, because there is nothing left to configure.
    assert!(drain_test_controls(&mut controls).is_empty());
}

#[test]
fn resize_timeout_restores_the_complete_committed_rectangle() {
    let surface = SurfaceId::new(6, 1);
    let committed = Rect {
        x: 647,
        y: 21,
        width: 1276,
        height: 1422,
    };
    let rejected = Rect {
        x: 7,
        y: 21,
        width: 1000,
        height: 700,
    };
    let transaction = TransactionId::from_raw(12);
    let mut layout = PersistentLiveLayout::default();
    register_test_routes(&mut layout, &[surface]);
    layout.layers.insert(surface, test_layer(surface, committed));
    layout.layout_epochs.record_committed(
        surface,
        Size {
            width: committed.width,
            height: committed.height,
        },
    );
    layout.pending = Some(PendingLiveWmLayout {
        transaction,
        layers: vec![test_layer(surface, rejected)],
        requested_sizes: BTreeMap::from([(
            surface,
            Size {
                width: rejected.width,
                height: rejected.height,
            },
        )]),
        presentation_states: BTreeMap::new(),
        presentation_settlements: BTreeSet::new(),
        configure_deliveries: 1,
        focus: Some(surface),
        deadline: Instant::now(),
        update: sophia_engine::WmTransactionUpdate {
            commit: TransactionCommit {
                transaction,
                outcome: TransactionOutcome::Committed,
                applied_surfaces: vec![surface],
            },
        },
        moved_surfaces: 1,
        staged_transactions: BTreeMap::new(),
        admission_surfaces: BTreeSet::new(),
        source: Some(LiveWmProposalSource::Action(WmActionId::from_raw(3))),
        policy_settlement: None,
    });
    let mut controls = crate::session_control::SessionControlQueue::default();

    let result = layout.expire_pending(&mut controls).unwrap().unwrap();
    assert_eq!(result.update.commit.outcome, TransactionOutcome::TimedOut);
    let commands = drain_test_controls(&mut controls);
    assert_eq!(commands.len(), 1);
    assert_eq!(
        commands[0].command,
        sophia_x_authority::XAuthorityControlCommand::ConfigureSurface {
            transaction: commands[0].command.transaction(),
            surface,
            geometry: committed,
        }
    );
}

#[test]
fn public_presentation_state_waits_for_frontend_acknowledgement() {
    let surface = SurfaceId::new(8, 1);
    let transaction = TransactionId::from_raw(18);
    let geometry = Rect {
        x: 0,
        y: 0,
        width: 1280,
        height: 720,
    };
    let state = sophia_protocol::PolicyPresentationState {
        maximized: true,
        ..sophia_protocol::PolicyPresentationState::default()
    };
    let mut layout = PersistentLiveLayout::default();
    register_test_routes(&mut layout, &[surface]);
    layout.layers.insert(surface, test_layer(surface, geometry));
    layout.layout_epochs.record_committed(
        surface,
        Size {
            width: geometry.width,
            height: geometry.height,
        },
    );
    let proposal = LiveWmProposal {
        transaction,
        layers: vec![test_layer(surface, geometry)],
        requested_sizes: BTreeMap::new(),
        presentation_states: BTreeMap::from([(surface, state)]),
        configure_deliveries: 0,
        focus: Some(surface),
        timeout: Duration::from_secs(1),
        update: sophia_engine::WmTransactionUpdate {
            commit: TransactionCommit {
                transaction,
                outcome: TransactionOutcome::Committed,
                applied_surfaces: vec![surface],
            },
        },
        moved_surfaces: 0,
        source: None,
        policy_settlement: None,
    };
    let mut controls = crate::session_control::SessionControlQueue::default();

    assert!(layout.stage(proposal, &mut controls).unwrap().is_none());
    assert!(!layout.pending_is_ready());
    let commands = drain_test_controls(&mut controls);
    assert_eq!(commands.len(), 1);
    assert_eq!(
        commands[0].command,
        sophia_x_authority::XAuthorityControlCommand::SetPresentationState {
            transaction,
            surface,
            state,
        }
    );
    assert!(layout.acknowledge_presentation_control(transaction, surface));
    assert!(layout.pending_is_ready());
    assert!(layout.resolve_pending().is_some());
    assert_eq!(
        layout.committed_policy_presentations.get(&surface),
        Some(&state)
    );
}

#[test]
fn rejected_presentation_state_restores_the_last_frontend_value() {
    let surface = SurfaceId::new(9, 1);
    let transaction = TransactionId::from_raw(19);
    let geometry = Rect {
        x: 0,
        y: 0,
        width: 1280,
        height: 720,
    };
    let previous = sophia_protocol::PolicyPresentationState {
        fullscreen: true,
        ..sophia_protocol::PolicyPresentationState::default()
    };
    let rejected = sophia_protocol::PolicyPresentationState {
        minimized: true,
        ..sophia_protocol::PolicyPresentationState::default()
    };
    let mut layout = PersistentLiveLayout::default();
    register_test_routes(&mut layout, &[surface]);
    layout.layers.insert(surface, test_layer(surface, geometry));
    layout
        .committed_policy_presentations
        .insert(surface, previous);
    layout.pending = Some(PendingLiveWmLayout {
        transaction,
        layers: Vec::new(),
        requested_sizes: BTreeMap::new(),
        presentation_states: BTreeMap::from([(surface, rejected)]),
        presentation_settlements: BTreeSet::new(),
        configure_deliveries: 0,
        focus: None,
        deadline: Instant::now(),
        update: sophia_engine::WmTransactionUpdate {
            commit: TransactionCommit {
                transaction,
                outcome: TransactionOutcome::Committed,
                applied_surfaces: vec![surface],
            },
        },
        moved_surfaces: 0,
        staged_transactions: BTreeMap::new(),
        admission_surfaces: BTreeSet::new(),
        source: None,
        policy_settlement: None,
    });
    let mut controls = crate::session_control::SessionControlQueue::default();

    let result = layout.expire_pending(&mut controls).unwrap().unwrap();
    assert_eq!(result.update.commit.outcome, TransactionOutcome::TimedOut);
    let commands = drain_test_controls(&mut controls);
    assert_eq!(commands.len(), 1);
    assert_eq!(
        commands[0].command,
        sophia_x_authority::XAuthorityControlCommand::RestorePresentationState {
            transaction,
            surface,
            state: previous,
        }
    );
    assert_eq!(
        layout.committed_policy_presentations.get(&surface),
        Some(&previous)
    );
}

#[test]
fn a_pixel_silent_surface_does_not_gate_a_sibling_resize() {
    // A launching client has no pixels to resize. It can pass the epoch's
    // exact-size gate only by drawing its first frame at precisely the
    // requested extent, inside a deadline the blind WM sizes for settled
    // clients. Holding the epoch on it does not make it answer sooner — it
    // fails the sibling that could have answered.
    let settled = SurfaceId::new(8, 1);
    let launching = SurfaceId::new(9, 1);
    let whole = Size {
        width: 2558,
        height: 1424,
    };
    let split = Size {
        width: 1278,
        height: 1424,
    };
    let settled_geometry = Rect {
        x: 0,
        y: 14,
        width: whole.width,
        height: whole.height,
    };
    let split_geometry = Rect {
        width: split.width,
        ..settled_geometry
    };
    let launching_geometry = Rect {
        x: 1280,
        ..split_geometry
    };
    let mut layout = PersistentLiveLayout::default();
    register_test_routes(&mut layout, &[settled, launching]);
    layout
        .layers
        .insert(settled, test_layer(settled, settled_geometry));
    layout.layout_epochs.record_committed(settled, whole);
    // `launching` is deliberately given no committed size and no safe
    // observation: it is a client that has never presented.

    let transaction = TransactionId::from_raw(21);
    let proposal = LiveWmProposal {
        transaction,
        layers: vec![
            test_layer(settled, split_geometry),
            test_layer(launching, launching_geometry),
        ],
        requested_sizes: BTreeMap::from([(settled, split), (launching, split)]),
        presentation_states: BTreeMap::new(),
        configure_deliveries: 0,
        focus: Some(settled),
        timeout: Duration::from_millis(300),
        update: sophia_engine::WmTransactionUpdate {
            commit: TransactionCommit {
                transaction,
                outcome: TransactionOutcome::Committed,
                applied_surfaces: vec![settled, launching],
            },
        },
        moved_surfaces: 2,
        source: Some(LiveWmProposalSource::Relayout),
        policy_settlement: None,
    };
    let mut controls = crate::session_control::SessionControlQueue::default();

    assert!(layout.stage(proposal, &mut controls).unwrap().is_none());

    // The epoch waits only on the surface that can answer it.
    assert_eq!(
        layout.pending.as_ref().unwrap().requested_sizes,
        BTreeMap::from([(settled, split)])
    );
    // The deferred surface keeps its target as a standing obligation, so the
    // extent is driven once it has pixels rather than being dropped.
    assert_eq!(layout.layout_epochs.pending_target(launching), Some(split));
}

#[test]
fn a_deferred_surface_spends_no_admission_retry() {
    // Two expiries retire an admission. A retry is spent by a surface that was
    // asked and did not answer, so an epoch a deferred surface took no part in
    // must not count against it — otherwise a launching client is withdrawn
    // for a deadline that was never its to meet.
    let settled = SurfaceId::new(8, 1);
    let launching = SurfaceId::new(9, 1);
    let whole = Size {
        width: 2558,
        height: 1424,
    };
    let split = Size {
        width: 1278,
        height: 1424,
    };
    let settled_geometry = Rect {
        x: 0,
        y: 14,
        width: whole.width,
        height: whole.height,
    };
    let split_geometry = Rect {
        width: split.width,
        ..settled_geometry
    };
    let launching_geometry = Rect {
        x: 1280,
        ..split_geometry
    };
    let transaction = TransactionId::from_raw(21);
    let mut layout = PersistentLiveLayout::default();
    register_test_routes(&mut layout, &[settled, launching]);
    layout
        .layers
        .insert(settled, test_layer(settled, settled_geometry));
    layout
        .layers
        .insert(launching, test_layer(launching, launching_geometry));
    layout.layout_epochs.record_committed(settled, whole);
    layout.unmanaged_surfaces.insert(launching);
    layout.pending = Some(PendingLiveWmLayout {
        transaction,
        layers: vec![
            test_layer(settled, split_geometry),
            test_layer(launching, launching_geometry),
        ],
        // The deferred surface is absent from the gate, exactly as `stage`
        // leaves it.
        requested_sizes: BTreeMap::from([(settled, split)]),
        presentation_states: BTreeMap::new(),
        presentation_settlements: BTreeSet::new(),
        configure_deliveries: 1,
        focus: Some(settled),
        deadline: Instant::now(),
        update: sophia_engine::WmTransactionUpdate {
            commit: TransactionCommit {
                transaction,
                outcome: TransactionOutcome::Committed,
                applied_surfaces: vec![settled],
            },
        },
        moved_surfaces: 2,
        staged_transactions: BTreeMap::new(),
        admission_surfaces: BTreeSet::from([launching]),
        source: Some(LiveWmProposalSource::Relayout),
        policy_settlement: None,
    });
    let mut controls = crate::session_control::SessionControlQueue::default();

    let result = layout.expire_pending(&mut controls).unwrap().unwrap();
    assert_eq!(result.update.commit.outcome, TransactionOutcome::TimedOut);
    assert_eq!(layout.admission_retries.get(&launching), None);
}

#[test]
fn an_exhausted_admission_records_the_surface_it_withdrew() {
    // Withdrawal used to be silent: the coordinator erased an admission with
    // nothing on the record, so a launch waiting on that surface went on
    // waiting for something that no longer existed.
    let surface = SurfaceId::new(9, 1);
    let size = Size {
        width: 1278,
        height: 1424,
    };
    let geometry = Rect {
        x: 1280,
        y: 14,
        width: size.width,
        height: size.height,
    };
    let transaction = TransactionId::from_raw(21);
    let mut layout = PersistentLiveLayout::default();
    register_test_routes(&mut layout, &[surface]);
    layout.layers.insert(surface, test_layer(surface, geometry));
    layout.unmanaged_surfaces.insert(surface);
    // One retry already spent, so this expiry is the terminal one.
    layout.admission_retries.insert(surface, 1);
    layout.pending = Some(PendingLiveWmLayout {
        transaction,
        layers: vec![test_layer(surface, geometry)],
        requested_sizes: BTreeMap::from([(surface, size)]),
        presentation_states: BTreeMap::new(),
        presentation_settlements: BTreeSet::new(),
        configure_deliveries: 1,
        focus: Some(surface),
        deadline: Instant::now(),
        update: sophia_engine::WmTransactionUpdate {
            commit: TransactionCommit {
                transaction,
                outcome: TransactionOutcome::Committed,
                applied_surfaces: vec![surface],
            },
        },
        moved_surfaces: 1,
        staged_transactions: BTreeMap::new(),
        admission_surfaces: BTreeSet::from([surface]),
        source: Some(LiveWmProposalSource::Relayout),
        policy_settlement: None,
    });
    let mut controls = crate::session_control::SessionControlQueue::default();

    layout.expire_pending(&mut controls).unwrap().unwrap();

    // The surface is named once, for whoever was waiting on it.
    assert_eq!(layout.take_withdrawn_admissions(), vec![surface]);
    // Draining it is what makes the report single-shot.
    assert!(layout.take_withdrawn_admissions().is_empty());
}
