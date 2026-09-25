use std::sync::Arc;
#[test]
fn pixel_silent_admission_retries_then_withdraws_without_an_owner_error() {
    let surface = SurfaceId::new(4, 1);
    let transaction = TransactionId::from_raw(4);
    let target = Rect {
        x: 0,
        y: 0,
        width: 900,
        height: 700,
    };
    let mut layout = PersistentLiveLayout::default();
    layout.unmanaged_surfaces.insert(surface);
    layout.pending = Some(PendingLiveWmLayout {
        transaction,
        layers: vec![test_layer(surface, target)],
        requested_sizes: BTreeMap::from([(
            surface,
            Size {
                width: target.width,
                height: target.height,
            },
        )]),
        presentation_states: BTreeMap::new(),
        presentation_settlements: BTreeSet::new(),
        configure_deliveries: 0,
        focus: Some(surface),
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
        admission_surfaces: BTreeSet::from([surface]),
        source: Some(LiveWmProposalSource::Manage(surface)),
        policy_settlement: None,
    });
    let mut controls = crate::session_control::SessionControlQueue::default();

    let result = layout.expire_pending(&mut controls).unwrap().unwrap();

    assert_eq!(result.update.commit.outcome, TransactionOutcome::TimedOut);
    assert_eq!(result.source, Some(LiveWmProposalSource::Manage(surface)));
    assert_eq!(layout.admission_retries.get(&surface), Some(&1));
    assert_eq!(
        layout.layout_epochs.pending_target(surface),
        Some(Size {
            width: target.width,
            height: target.height,
        })
    );
    assert_eq!(layout.layout_epochs.recovery_extent(surface), None);
    assert_eq!(controls.pending_len(), 0);
    assert!(layout.unmanaged_surfaces.contains(&surface));

    let mut routed =
        crate::live_session::wm_update_coordinator_batch(TransactionId::from_raw(5));
    routed.client = Some(sophia_x_authority::XServerFrontendClientId::from_raw(1));
    routed
        .presentation_intents
        .push(sophia_protocol::SurfacePresentationIntent {
            surface,
            kind: sophia_protocol::SurfacePresentationIntentKind::Request,
            role: sophia_protocol::SurfacePresentationRole::PolicyManaged,
            surface_kind: sophia_protocol::LayoutNodeKind::Toplevel,
            placement_preference: sophia_protocol::SurfacePlacementPreference::Default,
            presentation_owner: None,
            stack_rank: 0,
            geometry: target,
            constraints: SurfaceConstraints {
                min_size: None,
                max_size: None,
            },
            generation: 1,
        });
    routed.surface_routes.push(
        sophia_x_authority::XAuthoritySurfaceRouteObservation {
            surface,
            client: sophia_x_authority::XServerFrontendClientId::from_raw(1),
            admission: None,
        },
    );
    layout.client_routes.observe(&routed).unwrap();
    layout.pending = Some(PendingLiveWmLayout {
        transaction: TransactionId::from_raw(5),
        layers: vec![test_layer(surface, target)],
        requested_sizes: BTreeMap::from([(
            surface,
            Size {
                width: target.width,
                height: target.height,
            },
        )]),
        presentation_states: BTreeMap::new(),
        presentation_settlements: BTreeSet::new(),
        configure_deliveries: 0,
        focus: Some(surface),
        deadline: Instant::now(),
        update: sophia_engine::WmTransactionUpdate {
            commit: TransactionCommit {
                transaction: TransactionId::from_raw(5),
                outcome: TransactionOutcome::Committed,
                applied_surfaces: vec![surface],
            },
        },
        moved_surfaces: 0,
        staged_transactions: BTreeMap::new(),
        admission_surfaces: BTreeSet::from([surface]),
        source: Some(LiveWmProposalSource::Manage(surface)),
        policy_settlement: None,
    });

    let withdrawal = layout.expire_pending(&mut controls).unwrap().unwrap();

    assert_eq!(
        withdrawal.update.commit.outcome,
        TransactionOutcome::TimedOut
    );
    assert_eq!(controls.pending_len(), 1);
    assert!(!layout.unmanaged_surfaces.contains(&surface));
    assert!(!layout.admission_retries.contains_key(&surface));
    assert_eq!(layout.layout_epochs.pending_target(surface), None);
}

#[test]
fn admitted_pixels_cross_the_visual_boundary_once_at_planned_geometry() {
    let surface = SurfaceId::new(6, 1);
    let client = sophia_x_authority::XServerFrontendClientId::from_raw(1);
    let geometry = Rect {
        x: 10,
        y: 15,
        width: 640,
        height: 480,
    };
    let constraints = SurfaceConstraints {
        min_size: None,
        max_size: None,
    };
    let pixels = SurfaceTransaction {
        input_region: None,
        transaction: TransactionId::from_raw(12),
        authority: sophia_protocol::AuthorityKind::SophiaX,
        surface,
        namespace: None,
        target_geometry: Rect {
            x: 0,
            y: 0,
            ..geometry
        },
        presentation_extent: sophia_protocol::Size {
            width: geometry.width,
            height: geometry.height,
        },
        content: sophia_protocol::SurfaceContentSet::singleton(BufferSource::DmaBuf { handle: 45 }, sophia_protocol::Size {
            width: geometry.width,
            height: geometry.height,
        }),

        damage: Region::single(Rect {
            x: 0,
            y: 0,
            width: geometry.width,
            height: geometry.height,
        }),
        readiness: sophia_protocol::SurfaceTransactionReadiness::Ready,
        timeout_msec: 250,
        previous_committed_generation: 3,
    };
    let mut observed =
        crate::live_session::wm_update_coordinator_batch(TransactionId::from_raw(12));
    observed.client = Some(client);
    add_test_surface_route(&mut observed, surface, client);
    observed.surface_presentations.push(
        sophia_x_authority::XAuthoritySurfacePresentationObservation {
            surface,
            role: sophia_protocol::SurfacePresentationRole::PolicyManaged,
            kind: sophia_protocol::LayoutNodeKind::Toplevel,
            placement_preference: sophia_protocol::SurfacePlacementPreference::Default,
            owner: None,
            stack_rank: 0,
            mapped: false,
            geometry,
            constraints,
            generation: 1,
        },
    );
    observed
        .presentation_intents
        .push(sophia_protocol::SurfacePresentationIntent {
            surface,
            kind: sophia_protocol::SurfacePresentationIntentKind::Request,
            role: sophia_protocol::SurfacePresentationRole::PolicyManaged,
            surface_kind: sophia_protocol::LayoutNodeKind::Toplevel,
            placement_preference: sophia_protocol::SurfacePlacementPreference::Default,
            presentation_owner: None,
            stack_rank: 0,
            geometry,
            constraints,
            generation: 1,
        });
    observed.transactions.push(pixels);
    observed
        .present_submissions
        .push(sophia_x_authority::XAuthorityPresentSubmission {
            transaction: TransactionId::from_raw(12),
            surface,
            buffer: sophia_protocol::BufferHandle::from_raw(45),
            x_offset: 0,
            y_offset: 0,
            acquire_fence: None,
            idle_fence: None,
        });
    let mut layout = PersistentLiveLayout::default();
    layout.observe_authority_batch(&observed);
    let transaction = TransactionId::from_raw(13);
    let proposal = LiveWmProposal {
        transaction,
        layers: planning_layers_for(&layout, [surface]),
        requested_sizes: BTreeMap::from([(
            surface,
            Size {
                width: geometry.width,
                height: geometry.height,
            },
        )]),
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
        source: None,
        policy_settlement: None,
    };
    let mut controls = crate::session_control::SessionControlQueue::default();

    assert!(layout.stage(proposal, &mut controls).unwrap().is_none());
    assert_eq!(controls.pending_len(), 1);
    assert!(layout.acknowledge_admission_control(transaction, surface));
    assert!(matches!(
        crate::live_session::reconcile_live_layout_progress(&mut layout, false),
        crate::live_session::LiveLayoutProgress::DeferredReady
    ));
    assert!(layout.pending.is_some());
    assert!(matches!(
        crate::live_session::reconcile_live_layout_progress(&mut layout, true),
        crate::live_session::LiveLayoutProgress::Committed(_)
    ));
    // Admission can resolve in the same owner iteration that still carries
    // the original observation. The released group must replace, not
    // duplicate, that observation at production intake.
    let (projected, released) = layout.projected_batch(&observed);

    assert!(projected.transactions.is_empty());
    assert!(projected.present_submissions.is_empty());
    assert!(released.is_empty());
    layout.unmanaged_surfaces.remove(&surface);
    let empty =
        crate::live_session::wm_update_coordinator_batch(TransactionId::from_raw(14));
    let (_, released) = layout.projected_batch(&empty);
    assert_eq!(released.len(), 1);
    assert_eq!(released[0].transactions.len(), 1);
    assert_eq!(released[0].transactions[0].surface, surface);
    assert_eq!(released[0].transactions[0].target_geometry, geometry);
    assert_eq!(released[0].transactions[0].previous_committed_generation, 0);
    assert_eq!(released[0].present_submissions.len(), 1);
    assert_eq!(
        released[0].present_submissions[0].transaction,
        TransactionId::from_raw(12)
    );
    assert_eq!(
        layout.layers[&surface].source,
        BufferSource::DmaBuf { handle: 45 }
    );
    assert!(layout.pre_admission_groups.is_empty());
    assert!(layout.released_admission_groups.is_empty());
}

#[test]
fn released_admission_precedes_newer_same_surface_current_batch() {
    let surface = SurfaceId::new(60, 1);
    let geometry = Rect {
        x: 0,
        y: 0,
        width: 640,
        height: 480,
    };
    let transaction =
        |transaction, previous_committed_generation, target_buffer| SurfaceTransaction {
            input_region: None,
            transaction,
            authority: sophia_protocol::AuthorityKind::SophiaX,
            surface,
            namespace: None,
            target_geometry: geometry,
            presentation_extent: Size {
                width: (geometry).width,
                height: (geometry).height,
            },
            content: sophia_protocol::SurfaceContentSet::singleton(
                target_buffer,
                Size {
                    width: geometry.width,
                    height: geometry.height,
                },
            ),
            damage: Region::single(geometry),
            readiness: sophia_protocol::SurfaceTransactionReadiness::Ready,
            timeout_msec: 250,
            previous_committed_generation,
        };
    let admitted_transaction = TransactionId::from_raw(367);
    let current_transaction = TransactionId::from_raw(858);
    let mut current =
        crate::live_session::wm_update_coordinator_batch(current_transaction);
    current.transactions.push(transaction(
        current_transaction,
        1,
        BufferSource::CpuBuffer { handle: 858 },
    ));
    let released = [crate::live_session::LiveAdmissionAuthorityGroup {
        transaction: admitted_transaction,
        transactions: vec![transaction(
            admitted_transaction,
            0,
            BufferSource::DmaBuf { handle: 367 },
        )],
        cpu_buffer_updates: Vec::new(),
        present_submissions: vec![sophia_x_authority::XAuthorityPresentSubmission {
            transaction: admitted_transaction,
            surface,
            buffer: sophia_protocol::BufferHandle::from_raw(367),
            x_offset: 0,
            y_offset: 0,
            acquire_fence: None,
            idle_fence: None,
        }],
        software_present_submissions: Vec::new(),
        superseded: false,
    }];
    let production = crate::live_session::production_authority_batch(
        &current,
        &released,
        &PersistentLiveLayout::default(),
    )
    .unwrap();

    production.validate().unwrap();
    assert_eq!(
        production
            .groups
            .iter()
            .map(|group| group.transaction)
            .collect::<Vec<_>>(),
        vec![admitted_transaction, current_transaction]
    );
    assert_eq!(
        production.groups[0].transactions[0].transaction,
        admitted_transaction
    );
    assert_eq!(
        production.groups[1].transactions[0].transaction,
        current_transaction
    );
    assert_eq!(
        production.groups[0].present_submissions[0].transaction,
        admitted_transaction
    );

    let output = sophia_engine::HeadlessOutput {
        id: sophia_protocol::OutputId::from_raw(1),
        size: Size {
            width: 640,
            height: 480,
        },
        scale: 1,
    };
    let mut runtime =
        sophia_backend_live::LiveProductionVisualRuntime::new(&[output], None).unwrap();
    let prepared = runtime
        .prepare_authority_groups(&production.groups)
        .unwrap();
    assert_eq!(prepared.authority_commits.len(), 2);
    assert!(
        prepared
            .authority_commits
            .iter()
            .all(|commit| { commit.outcome == sophia_protocol::TransactionOutcome::Committed })
    );
    assert_eq!(
        runtime
            .committed_surfaces()
            .iter()
            .find(|state| state.surface == surface)
            .map(|state| state.committed_generation),
        Some(2)
    );
}

#[test]
fn recovered_awaiting_pixels_admission_releases_its_present_at_commit() {
    let surface = SurfaceId::new(7, 1);
    let client = sophia_x_authority::XServerFrontendClientId::from_raw(1);
    let geometry = Rect {
        x: 10,
        y: 15,
        width: 640,
        height: 480,
    };
    let pixel_transaction = TransactionId::from_raw(20);
    let buffer = sophia_protocol::BufferHandle::from_raw(21);
    let pixels = SurfaceTransaction {
        input_region: None,
        transaction: pixel_transaction,
        authority: sophia_protocol::AuthorityKind::SophiaX,
        surface,
        namespace: None,
        target_geometry: geometry,
        presentation_extent: Size {
            width: (geometry).width,
            height: (geometry).height,
        },
        content: sophia_protocol::SurfaceContentSet::singleton(BufferSource::DmaBuf {
            handle: buffer.raw(),
        }, sophia_protocol::Size {
            width: geometry.width,
            height: geometry.height,
        }),

        damage: Region::single(Rect {
            x: 0,
            y: 0,
            width: geometry.width,
            height: geometry.height,
        }),
        readiness: sophia_protocol::SurfaceTransactionReadiness::Ready,
        timeout_msec: 250,
        previous_committed_generation: 0,
    };
    let intent = sophia_protocol::SurfacePresentationIntent {
        surface,
        kind: sophia_protocol::SurfacePresentationIntentKind::Request,
        role: sophia_protocol::SurfacePresentationRole::PolicyManaged,
        surface_kind: sophia_protocol::LayoutNodeKind::Toplevel,
        placement_preference: sophia_protocol::SurfacePlacementPreference::Default,
        presentation_owner: None,
        stack_rank: 0,
        geometry,
        constraints: SurfaceConstraints {
            min_size: None,
            max_size: None,
        },
        generation: 1,
    };
    let mut observed =
        crate::live_session::wm_update_coordinator_batch(pixel_transaction);
    observed.client = Some(client);
    add_test_surface_route(&mut observed, surface, client);
    observed.presentation_intents.push(intent);
    observed.surface_presentations.push(
        sophia_x_authority::XAuthoritySurfacePresentationObservation {
            surface,
            role: intent.role,
            kind: intent.surface_kind,
            placement_preference: intent.placement_preference,
            owner: None,
            stack_rank: intent.stack_rank,
            // Admission is an engine lifecycle, not a mutable X mapped-bit
            // predicate. Pixels must remain quarantined even if X already
            // reports the window mapped.
            mapped: true,
            geometry,
            constraints: intent.constraints,
            generation: intent.generation,
        },
    );
    observed.transactions.push(pixels);
    observed
        .present_submissions
        .push(sophia_x_authority::XAuthorityPresentSubmission {
            transaction: pixel_transaction,
            surface,
            buffer,
            x_offset: 0,
            y_offset: 0,
            acquire_fence: None,
            idle_fence: None,
        });
    observed.released_dma_bufs.push(buffer);
    let mut layout = PersistentLiveLayout::default();
    layout.observe_authority_batch(&observed);
    let original_admission = TransactionId::from_raw(22);
    assert!(
        layout
            .admissions
            .begin_control(surface, original_admission, geometry)
    );
    assert!(
        layout
            .admissions
            .acknowledge_control(surface, original_admission)
    );
    let recovery_transaction = TransactionId::from_raw(23);
    let proposal = LiveWmProposal {
        transaction: recovery_transaction,
        layers: planning_layers_for(&layout, [surface]),
        requested_sizes: BTreeMap::from([(
            surface,
            Size {
                width: geometry.width,
                height: geometry.height,
            },
        )]),
        presentation_states: BTreeMap::new(),
        configure_deliveries: 0,
        focus: Some(surface),
        timeout: Duration::from_secs(1),
        update: sophia_engine::WmTransactionUpdate {
            commit: TransactionCommit {
                transaction: recovery_transaction,
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
    assert_eq!(
        layout
            .pending
            .as_ref()
            .map(|pending| &pending.admission_surfaces),
        Some(&BTreeSet::from([surface]))
    );
    assert!(layout.resolve_pending().is_some());
    let pixel_candidate = dma_candidate(pixel_transaction, surface, buffer);
    assert_eq!(
        layout.admissions.state(surface),
        sophia_engine::SurfacePresentationAdmissionState::AwaitingRetirement {
            admission_transaction: original_admission,
            visual_candidate: pixel_candidate,
            geometry,
        }
    );
    // The exact presented candidate is the only transition that may release it.
    assert_eq!(layout.focus_to_apply, None);
    let empty =
        crate::live_session::wm_update_coordinator_batch(TransactionId::from_raw(24));
    let (projected, released) = layout.projected_batch(&empty);
    assert!(projected.transactions.is_empty());
    assert!(projected.present_submissions.is_empty());
    assert!(released.is_empty());
    layout.unmanaged_surfaces.remove(&surface);
    let (projected, released) = layout.projected_batch(&empty);
    assert_eq!(released.len(), 1);
    assert_eq!(released[0].transactions.len(), 1);
    assert_eq!(released[0].transactions[0].transaction, pixel_transaction);
    assert_eq!(released[0].present_submissions.len(), 1);
    assert_eq!(
        released[0].present_submissions[0].transaction,
        pixel_transaction
    );
    assert!(projected.released_dma_bufs.is_empty());
    let (projected, released) = layout.projected_batch(&empty);
    assert!(released.is_empty());
    assert_eq!(projected.released_dma_bufs, vec![buffer]);
    layout.layout_epochs.set_recovery_extent(
        surface,
        Size {
            width: geometry.width,
            height: geometry.height,
        },
    );
    assert_eq!(layout.recovery_extent_count(), 1);
    assert_eq!(
        layout.layout_epochs.recovery_extent(surface),
        Some(Size {
            width: geometry.width,
            height: geometry.height,
        })
    );
    assert!(layout.complete_admission_retirement(pixel_candidate));
    assert_eq!(layout.recovery_extent_count(), 0);
    assert_eq!(layout.layout_epochs.recovery_extent(surface), None);
    assert!(layout.constraint_relayout_required());
    assert_eq!(
        layout.admissions.state(surface),
        sophia_engine::SurfacePresentationAdmissionState::Managed
    );
    assert_eq!(layout.focus_to_apply, Some((recovery_transaction, surface)));
}

#[test]
fn recovery_cannot_publish_admission_chrome_from_retained_size_without_pixels() {
    let surface = SurfaceId::new(71, 1);
    let client = sophia_x_authority::XServerFrontendClientId::from_raw(1);
    let geometry = Rect {
        x: 20,
        y: 30,
        width: 500,
        height: 500,
    };
    let intent = sophia_protocol::SurfacePresentationIntent {
        surface,
        kind: sophia_protocol::SurfacePresentationIntentKind::Request,
        role: sophia_protocol::SurfacePresentationRole::PolicyManaged,
        surface_kind: sophia_protocol::LayoutNodeKind::Toplevel,
        placement_preference: sophia_protocol::SurfacePlacementPreference::Default,
        presentation_owner: None,
        stack_rank: 0,
        geometry,
        constraints: SurfaceConstraints {
            min_size: None,
            max_size: None,
        },
        generation: 1,
    };
    let mut observed =
        crate::live_session::wm_update_coordinator_batch(TransactionId::from_raw(70));
    observed.client = Some(client);
    add_test_surface_route(&mut observed, surface, client);
    observed.presentation_intents.push(intent);
    observed.surface_presentations.push(
        sophia_x_authority::XAuthoritySurfacePresentationObservation {
            surface,
            role: intent.role,
            kind: intent.surface_kind,
            placement_preference: intent.placement_preference,
            owner: None,
            stack_rank: intent.stack_rank,
            mapped: true,
            geometry,
            constraints: intent.constraints,
            generation: intent.generation,
        },
    );
    let mut layout = PersistentLiveLayout::default();
    layout.observe_authority_batch(&observed);
    let admission_transaction = TransactionId::from_raw(71);
    assert!(
        layout
            .admissions
            .begin_control(surface, admission_transaction, geometry)
    );
    assert!(
        layout
            .admissions
            .acknowledge_control(surface, admission_transaction)
    );
    let size = Size {
        width: geometry.width,
        height: geometry.height,
    };
    layout.layout_epochs.record_committed(surface, size);
    let recovery_transaction = TransactionId::from_raw(72);
    let proposal = LiveWmProposal {
        transaction: recovery_transaction,
        layers: planning_layers_for(&layout, [surface]),
        requested_sizes: BTreeMap::new(),
        presentation_states: BTreeMap::new(),
        configure_deliveries: 0,
        focus: Some(surface),
        timeout: Duration::from_secs(1),
        update: sophia_engine::WmTransactionUpdate {
            commit: TransactionCommit {
                transaction: recovery_transaction,
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
    assert_eq!(controls.pending_len(), 1);
    assert_eq!(
        layout
            .pending
            .as_ref()
            .map(|pending| &pending.admission_surfaces),
        Some(&BTreeSet::from([surface]))
    );
    assert!(layout.resolve_pending().is_none());
    assert!(layout.layers.is_empty());
    assert_eq!(layout.focus_to_apply, None);
    assert_eq!(
        layout.admissions.state(surface),
        sophia_engine::SurfacePresentationAdmissionState::AwaitingPixels {
            transaction: admission_transaction,
            geometry,
        }
    );
}

#[test]
fn selected_present_settles_older_present_group_without_committing_it() {
    let surface = SurfaceId::new(82, 1);
    let geometry = Rect {
        x: 0,
        y: 0,
        width: 500,
        height: 500,
    };
    let mut intent =
        crate::live_session::wm_update_coordinator_batch(TransactionId::from_raw(90));
    intent
        .presentation_intents
        .push(sophia_protocol::SurfacePresentationIntent {
            surface,
            kind: sophia_protocol::SurfacePresentationIntentKind::Request,
            role: sophia_protocol::SurfacePresentationRole::PolicyManaged,
            surface_kind: sophia_protocol::LayoutNodeKind::Toplevel,
            placement_preference: sophia_protocol::SurfacePlacementPreference::Default,
            presentation_owner: None,
            stack_rank: 0,
            geometry,
            constraints: SurfaceConstraints {
                min_size: None,
                max_size: None,
            },
            generation: 1,
        });
    let mut layout = PersistentLiveLayout::default();
    layout.observe_authority_batch(&intent);
    layout.layers.insert(surface, test_layer(surface, geometry));

    for raw in [91, 92] {
        let transaction = TransactionId::from_raw(raw);
        let buffer = sophia_protocol::BufferHandle::from_raw(raw);
        layout.dma_buf_sizes.insert(
            buffer,
            Size {
                width: geometry.width,
                height: geometry.height,
            },
        );
        let mut present = crate::live_session::wm_update_coordinator_batch(transaction);
        present.transactions.push(SurfaceTransaction {
            input_region: None,
            transaction,
            authority: sophia_protocol::AuthorityKind::SophiaX,
            surface,
            namespace: None,
            target_geometry: geometry,
            presentation_extent: Size {
                width: (geometry).width,
                height: (geometry).height,
            },
            content: sophia_protocol::SurfaceContentSet::singleton(BufferSource::DmaBuf {
                handle: buffer.raw(),
            }, sophia_protocol::Size {
                width: geometry.width,
                height: geometry.height,
            }),

            damage: Region::single(geometry),
            readiness: sophia_protocol::SurfaceTransactionReadiness::Ready,
            timeout_msec: 250,
            previous_committed_generation: raw - 91,
        });
        present
            .present_submissions
            .push(sophia_x_authority::XAuthorityPresentSubmission {
                transaction,
                surface,
                buffer,
                x_offset: 0,
                y_offset: 0,
                acquire_fence: None,
                idle_fence: None,
            });
        layout.observe_authority_batch(&present);
    }

    layout.release_admission_groups(&BTreeMap::from([(surface, TransactionId::from_raw(92))]));

    assert!(layout.pre_admission_groups.is_empty());
    assert_eq!(layout.released_admission_groups.len(), 2);
    assert!(layout.released_admission_groups[0].superseded);
    assert!(!layout.released_admission_groups[1].superseded);
    assert_eq!(
        layout.released_admission_groups[1].transactions[0].previous_committed_generation,
        0
    );
}

#[test]
fn pre_admission_group_with_mixed_transaction_identity_fails_closed() {
    let surface = SurfaceId::new(9, 1);
    let geometry = Rect {
        x: 0,
        y: 0,
        width: 500,
        height: 500,
    };
    let mut intent =
        crate::live_session::wm_update_coordinator_batch(TransactionId::from_raw(30));
    let client = sophia_x_authority::XServerFrontendClientId::from_raw(1);
    intent.client = Some(client);
    add_test_surface_route(&mut intent, surface, client);
    intent.surface_presentations.push(
        sophia_x_authority::XAuthoritySurfacePresentationObservation {
            surface,
            role: sophia_protocol::SurfacePresentationRole::PolicyManaged,
            kind: sophia_protocol::LayoutNodeKind::Toplevel,
            placement_preference: sophia_protocol::SurfacePlacementPreference::Default,
            stack_rank: 0,
            owner: None,
            mapped: false,
            geometry,
            constraints: SurfaceConstraints {
                min_size: None,
                max_size: None,
            },
            generation: 1,
        },
    );
    intent
        .presentation_intents
        .push(sophia_protocol::SurfacePresentationIntent {
            surface,
            kind: sophia_protocol::SurfacePresentationIntentKind::Request,
            role: sophia_protocol::SurfacePresentationRole::PolicyManaged,
            surface_kind: sophia_protocol::LayoutNodeKind::Toplevel,
            placement_preference: sophia_protocol::SurfacePlacementPreference::Default,
            presentation_owner: None,
            stack_rank: 0,
            geometry,
            constraints: SurfaceConstraints {
                min_size: None,
                max_size: None,
            },
            generation: 1,
        });
    let mut layout = PersistentLiveLayout::default();
    layout.observe_authority_batch(&intent);

    let mut malformed =
        crate::live_session::wm_update_coordinator_batch(TransactionId::from_raw(31));
    malformed
        .present_submissions
        .push(sophia_x_authority::XAuthorityPresentSubmission {
            transaction: TransactionId::from_raw(32),
            surface,
            buffer: sophia_protocol::BufferHandle::from_raw(1),
            x_offset: 0,
            y_offset: 0,
            acquire_fence: None,
            idle_fence: None,
        });
    let observation = layout.observe_authority_batch(&malformed);

    assert_eq!(
        observation.admission_group_error,
        Some("pre-admission authority group contains a mismatched Present")
    );
    assert!(layout.pre_admission_groups.is_empty());
}

#[test]
fn backing_admission_releases_cpu_replacement_before_selected_patch() {
    let surface = SurfaceId::new(10, 1);
    let geometry = Rect {
        x: 0,
        y: 0,
        width: 4,
        height: 4,
    };
    let size = Size {
        width: geometry.width,
        height: geometry.height,
    };
    let handle = 172;
    let base_transaction = TransactionId::from_raw(380);
    let selected_transaction = TransactionId::from_raw(381);
    let transaction = |transaction, previous_committed_generation| SurfaceTransaction {
        input_region: None,
        transaction,
        authority: sophia_protocol::AuthorityKind::SophiaX,
        surface,
        namespace: None,
        target_geometry: geometry,
        presentation_extent: Size {
            width: (geometry).width,
            height: (geometry).height,
        },
        content: sophia_protocol::SurfaceContentSet::singleton(BufferSource::CpuBuffer { handle }, sophia_protocol::Size {
            width: geometry.width,
            height: geometry.height,
        }),

        damage: Region::single(geometry),
        readiness: sophia_protocol::SurfaceTransactionReadiness::Ready,
        timeout_msec: 250,
        previous_committed_generation,
    };
    let base = sophia_x_authority::XAuthorityCpuBufferUpdate::Replace(
        sophia_x_authority::XAuthorityCpuBufferSnapshot {
            handle,
            drawable: sophia_x_authority::XResourceId::new(10, 1),
            size,
            stride: 16,
            format: 0,
            generation: 1,
            bytes: Arc::new(vec![0; 64]),
        },
    );
    let patch = sophia_x_authority::XAuthorityCpuBufferUpdate::PatchBatch(
        sophia_x_authority::XAuthorityCpuBufferPatchBatch {
            handle,
            drawable: sophia_x_authority::XResourceId::new(10, 1),
            size,
            stride: 16,
            format: 0,
            generation: 2,
            patches: vec![sophia_x_authority::XAuthorityCpuBufferPatchRegion {
                rect: geometry,
                bytes: vec![1; 64],
            }],
        },
    );
    let mut layout = PersistentLiveLayout::default();
    layout.layers.insert(surface, test_layer(surface, geometry));
    layout.layout_epochs.record_safe_observation(
        sophia_protocol::SurfaceTransactionKey {
            transaction: selected_transaction,
            surface,
            target_buffer: BufferSource::CpuBuffer { handle },
        },
        size,
        sophia_engine::SurfaceVisualEvidence::BackingSnapshot,
    );
    layout.pre_admission_groups.push_back(
        crate::live_session::LiveAdmissionAuthorityGroup {
            transaction: base_transaction,
            transactions: vec![transaction(base_transaction, 0)],
            cpu_buffer_updates: vec![base],
            present_submissions: Vec::new(),
            software_present_submissions: Vec::new(),
            superseded: false,
        },
    );
    layout.pre_admission_groups.push_back(
        crate::live_session::LiveAdmissionAuthorityGroup {
            transaction: selected_transaction,
            transactions: vec![transaction(selected_transaction, 1)],
            cpu_buffer_updates: vec![patch],
            present_submissions: Vec::new(),
            software_present_submissions: Vec::new(),
            superseded: false,
        },
    );

    layout.release_admission_groups(&BTreeMap::from([(
        surface,
        selected_transaction,
    )]));

    assert!(layout.pre_admission_groups.is_empty());
    assert_eq!(layout.released_admission_groups.len(), 2);
    assert!(matches!(
        layout.released_admission_groups[0].cpu_buffer_updates[0],
        sophia_x_authority::XAuthorityCpuBufferUpdate::Replace(_)
    ));
    assert!(matches!(
        layout.released_admission_groups[1].cpu_buffer_updates[0],
        sophia_x_authority::XAuthorityCpuBufferUpdate::PatchBatch(_)
    ));
    assert_eq!(
        layout.released_admission_groups[0].transactions[0]
            .previous_committed_generation,
        0
    );
    assert_eq!(
        layout.released_admission_groups[1].transactions[0]
            .previous_committed_generation,
        1
    );
}
