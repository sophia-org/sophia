use super::*;

#[test]
fn completed_pointer_geometry_reduces_raw_motion_to_one_bounded_target() {
    let initial = Rect {
        x: 100,
        y: 80,
        width: 300,
        height: 200,
    };
    let resize = sophia_protocol::WmPointerGestureCompleted {
        surface: SurfaceId::new(91, 1),
        output: OutputId::INVALID,
        workspace: sophia_protocol::WorkspaceId::INVALID,
        mode: sophia_protocol::WmPointerGestureMode::Resize,
        start: sophia_protocol::WmPointerPosition { x: 120, y: 100 },
        end: sophia_protocol::WmPointerPosition { x: 220, y: 50 },
    };
    assert_eq!(
        completed_pointer_gesture_geometry(resize, initial),
        Rect {
            width: 400,
            height: 150,
            ..initial
        }
    );
}

#[test]
fn public_pointer_updates_replace_only_the_latest_matching_update() {
    let surface = SurfaceId::new(91, 1);
    let source = LiveWmProposalSource::PointerGesture {
        surface,
        mode: sophia_protocol::WmPointerGestureMode::Move,
    };
    let cause = |phase, x| LivePublicPolicyCause {
        source,
        cause: sophia_protocol::PolicyRequestCause::Interaction {
            phase,
            kind: sophia_protocol::PolicyInteractionKind::Move,
            axis: sophia_protocol::PolicyInteractionAxis::None,
            target: surface,
            geometry: Rect {
                x,
                y: 20,
                width: 300,
                height: 200,
            },
        },
        affected_outputs: vec![OutputId::from_raw(1)],
    };
    let mut queue = VecDeque::new();

    assert_eq!(
        enqueue_public_policy_cause(
            &mut queue,
            None,
            false,
            cause(sophia_protocol::PolicyInteractionPhase::Begin, 10),
        ),
        LiveWmRequestAdmission::Admitted
    );
    assert_eq!(
        enqueue_public_policy_cause(
            &mut queue,
            Some(source),
            true,
            cause(sophia_protocol::PolicyInteractionPhase::Update, 20),
        ),
        LiveWmRequestAdmission::Admitted
    );
    assert_eq!(
        enqueue_public_policy_cause(
            &mut queue,
            Some(source),
            true,
            cause(sophia_protocol::PolicyInteractionPhase::Update, 30),
        ),
        LiveWmRequestAdmission::Duplicate
    );
    assert_eq!(queue.len(), 2);
    assert!(matches!(
        queue.back().map(|pending| &pending.cause),
        Some(sophia_protocol::PolicyRequestCause::Interaction {
            phase: sophia_protocol::PolicyInteractionPhase::Update,
            geometry: Rect { x: 30, .. },
            ..
        })
    ));
    assert_eq!(
        enqueue_public_policy_cause(
            &mut queue,
            Some(source),
            true,
            cause(sophia_protocol::PolicyInteractionPhase::End, 40),
        ),
        LiveWmRequestAdmission::Admitted
    );
    assert_eq!(queue.len(), 3);
}

#[test]
fn public_security_cancel_purges_stale_values_and_preempts_unrelated_work() {
    let surface = SurfaceId::new(92, 1);
    let source = LiveWmProposalSource::PointerGesture {
        surface,
        mode: sophia_protocol::WmPointerGestureMode::Resize,
    };
    let interaction = |phase, width| LivePublicPolicyCause {
        source,
        cause: sophia_protocol::PolicyRequestCause::Interaction {
            phase,
            kind: sophia_protocol::PolicyInteractionKind::Resize,
            axis: sophia_protocol::PolicyInteractionAxis::None,
            target: surface,
            geometry: Rect {
                x: 10,
                y: 20,
                width,
                height: 200,
            },
        },
        affected_outputs: vec![OutputId::from_raw(1)],
    };
    let mut queue = VecDeque::from([
        interaction(sophia_protocol::PolicyInteractionPhase::Begin, 300),
        LivePublicPolicyCause {
            source: LiveWmProposalSource::Action(WmActionId::from_raw(7)),
            cause: sophia_protocol::PolicyRequestCause::Action {
                activation_serial: 9,
                action: WmActionId::from_raw(7),
            },
            affected_outputs: vec![OutputId::from_raw(1)],
        },
        interaction(sophia_protocol::PolicyInteractionPhase::Update, 350),
        interaction(sophia_protocol::PolicyInteractionPhase::End, 400),
    ]);

    assert_eq!(
        enqueue_public_policy_security_cancel(
            &mut queue,
            true,
            interaction(sophia_protocol::PolicyInteractionPhase::Cancel, 350),
        ),
        LiveWmRequestAdmission::Admitted
    );
    assert_eq!(queue.len(), 2);
    assert!(matches!(
        queue.front().map(|pending| &pending.cause),
        Some(sophia_protocol::PolicyRequestCause::Interaction {
            phase: sophia_protocol::PolicyInteractionPhase::Cancel,
            ..
        })
    ));
    assert!(matches!(
        queue.back().map(|pending| pending.source),
        Some(LiveWmProposalSource::Action(_))
    ));
}
use crate::live_session::{
    LivePolicyMapMode, LivePolicySettlementIdentity, LivePublicPolicyCause, LiveWmProposal,
    LiveWmProposalSource, LiveWmRequestAdmission, PendingLiveWmLayout, PersistentLiveLayout,
    ResizeVisualCommit, enqueue_public_policy_cause, enqueue_public_policy_security_cancel,
    public_live_proposal, public_policy_surface_snapshots, reconcile_public_policy_proposal,
};
use sophia_protocol::{
    BufferHandle, SurfaceConstraints, TransactionCommit, TransactionId, TransactionOutcome,
    WorkspaceId,
};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

fn test_layer(surface: SurfaceId, geometry: Rect) -> LayerSnapshot {
    LayerSnapshot {
        input_region: None,
        translation: None,
        output: None,
        surface,
        authority_local_id: None,
        namespace: None,
        stack_rank: 0,
        geometry,
        source_size: Size {
            width: geometry.width,
            height: geometry.height,
        },
        source: BufferSource::None,
        damage: Region::single(geometry),
        opacity: 1.0,
        crop: None,
        transform: Transform::IDENTITY,
        generation: 1,
        resize_sync: ResizeSyncCapability::ImplicitOnly,
    }
}

fn dma_candidate(
    transaction: TransactionId,
    surface: SurfaceId,
    buffer: BufferHandle,
) -> sophia_protocol::SurfaceTransactionKey {
    sophia_protocol::SurfaceTransactionKey {
        transaction,
        surface,
        target_buffer: BufferSource::DmaBuf {
            handle: buffer.raw(),
        },
    }
}

fn test_live_layout_node(
    layer: &LayerSnapshot,
    workspace: WorkspaceId,
    coordinator: &sophia_engine::LayoutEpochCoordinator,
    chrome: sophia_engine::SurfaceChromeStyle,
) -> Result<sophia_protocol::LayoutNodeSnapshot, sophia_engine::ChromeLayoutError> {
    test_layout_node_from_facts(
        sophia_engine::SurfaceLayoutFacts {
            surface: layer.surface,
            role: sophia_protocol::SurfacePresentationRole::PolicyManaged,
            kind: sophia_protocol::LayoutNodeKind::Toplevel,
            placement_preference: sophia_protocol::SurfacePlacementPreference::Default,
            presentation_owner: None,
            stack_rank: layer.stack_rank,
            geometry: layer.geometry,
            constraints: coordinator.declared_constraints(layer.surface),
            generation: layer.generation,
        },
        workspace,
        coordinator,
        chrome,
    )
}

fn test_layout_node_from_facts(
    facts: sophia_engine::SurfaceLayoutFacts,
    workspace: WorkspaceId,
    coordinator: &sophia_engine::LayoutEpochCoordinator,
    chrome: sophia_engine::SurfaceChromeStyle,
) -> Result<sophia_protocol::LayoutNodeSnapshot, sophia_engine::ChromeLayoutError> {
    let mut capabilities = sophia_protocol::LayoutNodeCapabilities::STANDARD_TOPLEVEL;
    capabilities.resizable = coordinator.surface_declared_resizable(facts.surface);
    Ok(sophia_protocol::LayoutNodeSnapshot {
        surface: facts.surface,
        workspace,
        kind: facts.kind,
        placement_preference: facts.placement_preference,
        transient_owner: facts.presentation_owner,
        capabilities,
        state: sophia_protocol::LayoutNodeState {
            floating: facts.placement_preference
                == sophia_protocol::SurfacePlacementPreference::Floating,
            ..sophia_protocol::LayoutNodeState::NORMAL
        },
        constraints: sophia_engine::outer_surface_constraints(facts.constraints, chrome)?,
        geometry: sophia_engine::outer_surface_geometry(facts.geometry, chrome)?,
        generation: facts.generation,
    })
}

fn planning_layers_for(
    layout: &PersistentLiveLayout,
    surfaces: impl IntoIterator<Item = SurfaceId>,
) -> Vec<LayerSnapshot> {
    surfaces
        .into_iter()
        .filter_map(|surface| {
            layout.layers.get(&surface).cloned().or_else(|| {
                let facts = layout.layout_facts(surface)?;
                Some(LayerSnapshot {
                    input_region: None,
                    translation: None,
                    output: None,
                    surface: facts.surface,
                    authority_local_id: None,
                    namespace: None,
                    stack_rank: facts.stack_rank,
                    geometry: facts.geometry,
                    source: BufferSource::None,
                    source_size: Size {
                        width: facts.geometry.width,
                        height: facts.geometry.height,
                    },
                    damage: Region::empty(),
                    opacity: 1.0,
                    crop: None,
                    transform: Transform::IDENTITY,
                    generation: facts.generation,
                    resize_sync: ResizeSyncCapability::ImplicitOnly,
                })
            })
        })
        .collect()
}

#[test]
fn public_policy_snapshot_retains_an_admitted_surface_while_it_is_hidden() {
    let surface = SurfaceId::new(92, 4);
    let geometry = Rect {
        x: 24,
        y: 32,
        width: 640,
        height: 480,
    };
    let mut layout = PersistentLiveLayout::default();
    let mut observed = crate::live_session::wm_update_coordinator_batch(TransactionId::from_raw(1));
    let client = sophia_x_authority::XServerFrontendClientId::from_raw(1);
    observed.client = Some(client);
    add_test_surface_route(&mut observed, surface, client);
    observed.surface_presentations.push(
        sophia_x_authority::XAuthoritySurfacePresentationObservation {
            surface,
            role: sophia_protocol::SurfacePresentationRole::PolicyManaged,
            kind: sophia_protocol::LayoutNodeKind::Toplevel,
            placement_preference: sophia_protocol::SurfacePlacementPreference::Default,
            owner: None,
            stack_rank: 3,
            mapped: false,
            geometry,
            constraints: SurfaceConstraints {
                min_size: None,
                max_size: None,
            },
            generation: 7,
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
            stack_rank: 3,
            geometry,
            constraints: SurfaceConstraints {
                min_size: None,
                max_size: None,
            },
            generation: 7,
        });
    layout.observe_authority_batch(&observed);
    // Engine admission consumes planning ownership. The X frontend's
    // observation remains `mapped=false` because policy admission is not a
    // second client MapWindow request; neither fact ends policy ownership.
    layout.planning_surfaces.remove(&surface);

    let unrouted = SurfaceId::new(93, 1);
    let mut direct_observation =
        crate::live_session::wm_update_coordinator_batch(TransactionId::from_raw(2));
    direct_observation.surface_presentations.push(
        sophia_x_authority::XAuthoritySurfacePresentationObservation {
            surface: unrouted,
            role: sophia_protocol::SurfacePresentationRole::PolicyManaged,
            kind: sophia_protocol::LayoutNodeKind::Toplevel,
            placement_preference: sophia_protocol::SurfacePlacementPreference::Default,
            owner: None,
            stack_rank: 4,
            mapped: false,
            geometry,
            constraints: SurfaceConstraints {
                min_size: None,
                max_size: None,
            },
            generation: 1,
        },
    );
    layout.observe_authority_batch(&direct_observation);

    assert!(layout.layers.is_empty());
    assert!(layout.planning_surfaces.is_empty());
    assert!(!layout.mapped_surfaces.contains(&surface));
    let surfaces = public_policy_surface_snapshots(
        &layout,
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        sophia_engine::SurfaceChromeStyle::default(),
    )
    .unwrap();

    assert_eq!(surfaces.len(), 1);
    assert_eq!(surfaces[0].surface, surface);
    assert_eq!(surfaces[0].generation, 7);
    assert_eq!(surfaces[0].current_output, None);
    assert_eq!(
        surfaces[0].geometry,
        sophia_engine::outer_surface_geometry(
            geometry,
            sophia_engine::SurfaceChromeStyle::default(),
        )
        .unwrap()
    );

    // Pixel identity is not policy state. Once admission has a retained
    // raster layer, its content generation may advance independently of the X
    // authority's window-lifecycle generation and must not stale a public WM
    // request that depends on the latter.
    let mut repainted = test_layer(surface, geometry);
    repainted.generation = 91;
    layout.layers.insert(surface, repainted);
    let repainted_surfaces = public_policy_surface_snapshots(
        &layout,
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        sophia_engine::SurfaceChromeStyle::default(),
    )
    .unwrap();
    assert_eq!(repainted_surfaces[0].generation, 7);
    layout.layers.remove(&surface);

    let mut withdrawn =
        crate::live_session::wm_update_coordinator_batch(TransactionId::from_raw(3));
    withdrawn.surface_presentations.push(
        sophia_x_authority::XAuthoritySurfacePresentationObservation {
            mapped: false,
            ..observed.surface_presentations[0]
        },
    );
    withdrawn
        .presentation_intents
        .push(sophia_protocol::SurfacePresentationIntent {
            surface,
            kind: sophia_protocol::SurfacePresentationIntentKind::Withdraw,
            role: sophia_protocol::SurfacePresentationRole::PolicyManaged,
            surface_kind: sophia_protocol::LayoutNodeKind::Toplevel,
            placement_preference: sophia_protocol::SurfacePlacementPreference::Default,
            presentation_owner: None,
            stack_rank: 3,
            geometry,
            constraints: SurfaceConstraints {
                min_size: None,
                max_size: None,
            },
            generation: 7,
        });
    layout.observe_authority_batch(&withdrawn);
    assert!(
        public_policy_surface_snapshots(
            &layout,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            sophia_engine::SurfaceChromeStyle::default(),
        )
        .unwrap()
        .is_empty()
    );
}

#[test]
fn newer_committed_policy_replaces_deferred_retirement_focus() {
    let old_surface = SurfaceId::new(5, 1);
    let new_surface = SurfaceId::new(6, 1);
    let old_transaction = TransactionId::from_raw(18);
    let new_transaction = TransactionId::from_raw(19);
    let geometry = Rect {
        x: 0,
        y: 0,
        width: 1280,
        height: 720,
    };
    let mut layout = PersistentLiveLayout::default();
    layout.retirement_focus.insert(
        old_surface,
        (
            sophia_protocol::SurfaceTransactionKey {
                transaction: old_transaction,
                surface: old_surface,
                target_buffer: BufferSource::None,
            },
            old_transaction,
        ),
    );

    layout.commit_proposal(LiveWmProposal {
        transaction: new_transaction,
        layers: vec![test_layer(new_surface, geometry)],
        requested_sizes: BTreeMap::new(),
        presentation_states: BTreeMap::new(),
        configure_deliveries: 0,
        focus: Some(new_surface),
        timeout: Duration::from_secs(1),
        update: sophia_engine::WmTransactionUpdate {
            commit: TransactionCommit {
                transaction: new_transaction,
                outcome: TransactionOutcome::Committed,
                applied_surfaces: vec![new_surface],
            },
        },
        moved_surfaces: 0,
        source: Some(LiveWmProposalSource::Action(WmActionId::from_raw(1))),
        policy_settlement: None,
    });

    assert!(layout.retirement_focus.is_empty());
    assert_eq!(layout.focus_to_apply, Some((new_transaction, new_surface)));
}

#[test]
fn admission_does_not_prime_a_candidate_missing_from_quarantine() {
    let surface = SurfaceId::new(7, 1);
    let extent = Size {
        width: 500,
        height: 570,
    };
    let mut layout = PersistentLiveLayout::default();
    layout.unmanaged_surfaces.insert(surface);
    layout.presentation_roles.insert(
        surface,
        sophia_protocol::SurfacePresentationRole::PolicyManaged,
    );
    layout
        .admissions
        .observe_intent(sophia_protocol::SurfacePresentationIntent {
            surface,
            kind: sophia_protocol::SurfacePresentationIntentKind::Request,
            role: sophia_protocol::SurfacePresentationRole::PolicyManaged,
            surface_kind: sophia_protocol::LayoutNodeKind::Toplevel,
            placement_preference: sophia_protocol::SurfacePlacementPreference::Default,
            presentation_owner: None,
            stack_rank: 0,
            geometry: Rect {
                x: 0,
                y: 0,
                width: extent.width,
                height: extent.height,
            },
            constraints: SurfaceConstraints {
                min_size: None,
                max_size: None,
            },
            generation: 1,
        });
    layout
        .layout_epochs
        .set_admission(surface, sophia_engine::SurfaceAdmissionState::Unmanaged);
    layout.layout_epochs.record_safe_observation(
        dma_candidate(
            TransactionId::from_raw(20),
            surface,
            BufferHandle::from_raw(9),
        ),
        extent,
        sophia_engine::SurfaceVisualEvidence::PresentedBuffer,
    );

    let decision = layout.synchronize_admission_extent(surface);

    assert_eq!(
        decision,
        crate::resize_transaction::AdmissionRecoveryExtentDecision::AwaitingCandidate
    );
    assert_eq!(layout.layout_epochs.recovery_extent(surface), None);
    layout.layout_epochs.set_recovery_extent(surface, extent);
    assert_eq!(
        layout.synchronize_admission_extent(surface),
        crate::resize_transaction::AdmissionRecoveryExtentDecision::ClearStale { previous: extent }
    );
    assert_eq!(layout.layout_epochs.recovery_extent(surface), None);
    assert!(layout.constraint_relayout_required());
    assert_eq!(
        layout.layout_epochs.admission(surface),
        sophia_engine::SurfaceAdmissionState::Unmanaged
    );
}

#[test]
fn public_policy_admission_reconciles_to_the_engine_safe_extent_before_staging() {
    let output = OutputId::from_raw(1);
    let surface = SurfaceId::new(8, 1);
    let safe = Size {
        width: 1323,
        height: 1424,
    };
    let proposed = Size {
        width: 2560,
        height: 1440,
    };
    let mut layout = PersistentLiveLayout::default();
    layout.layout_epochs.set_recovery_extent(surface, safe);
    let proposal = sophia_protocol::PolicyProjectionProposal {
        launch_contexts: Vec::new(),
        output_launch_contexts: Vec::new(),
        translation_groups: Vec::new(),
        tab_groups: Vec::new(),
        transaction: TransactionId::from_raw(9),
        connection_epoch: 1,
        request_id: 1,
        base_generation: 1,
        active_output: output,
        outputs: vec![sophia_protocol::PolicyOutputProjection {
            output,
            placements: vec![sophia_protocol::PolicySurfacePlacement {
                surface,
                surface_generation: 1,
                geometry: Rect {
                    x: 0,
                    y: 0,
                    width: proposed.width,
                    height: proposed.height,
                },
                requested_size: Some(proposed),
                crop: None,
                transform: sophia_protocol::PolicyTransform::Identity,
                presentation: sophia_protocol::PolicyPresentationState::default(),
            }],
            focus: Some(surface),
        }],
        indicators: Vec::new(),
        output_statuses: Vec::new(),
    };

    let chrome = sophia_engine::SurfaceChromeStyle::default();
    let reconciled = reconcile_public_policy_proposal(
        &layout,
        &proposal,
        &BTreeMap::from([(
            output,
            Rect {
                x: 0,
                y: 0,
                width: proposed.width,
                height: proposed.height,
            },
        )]),
        &BTreeMap::from([(
            output,
            Rect {
                x: 0,
                y: 0,
                width: proposed.width,
                height: proposed.height,
            },
        )]),
        chrome,
    )
    .unwrap();

    assert_eq!(reconciled.adjusted_surfaces, 1);
    assert_eq!(
        reconciled.policy.outputs[0].placements[0].requested_size,
        Some(Size {
            width: safe.width + chrome.clearance() * 2,
            height: safe.height + chrome.clearance() * 2,
        })
    );
    assert_eq!(
        reconciled.content[&surface].geometry,
        Rect {
            x: chrome.clearance(),
            y: chrome.clearance(),
            width: safe.width,
            height: safe.height,
        }
    );
    assert_eq!(reconciled.content[&surface].requested_size, Some(safe));
    assert_eq!(
        proposal.outputs[0].placements[0].requested_size,
        Some(proposed)
    );
}

#[test]
fn public_policy_reconciliation_keeps_policy_omission_but_drives_changed_content() {
    let output = OutputId::from_raw(1);
    let surface = SurfaceId::new(9, 1);
    let geometry = Rect {
        x: 20,
        y: 30,
        width: 500,
        height: 400,
    };
    let proposal = sophia_protocol::PolicyProjectionProposal {
        launch_contexts: Vec::new(),
        output_launch_contexts: Vec::new(),
        translation_groups: Vec::new(),
        tab_groups: Vec::new(),
        transaction: TransactionId::from_raw(10),
        connection_epoch: 1,
        request_id: 1,
        base_generation: 1,
        active_output: output,
        outputs: vec![sophia_protocol::PolicyOutputProjection {
            output,
            placements: vec![sophia_protocol::PolicySurfacePlacement {
                surface,
                surface_generation: 1,
                geometry,
                requested_size: None,
                crop: None,
                transform: sophia_protocol::PolicyTransform::Identity,
                presentation: sophia_protocol::PolicyPresentationState::default(),
            }],
            focus: Some(surface),
        }],
        indicators: Vec::new(),
        output_statuses: Vec::new(),
    };

    let chrome = sophia_engine::SurfaceChromeStyle::default();
    let reconciled = reconcile_public_policy_proposal(
        &PersistentLiveLayout::default(),
        &proposal,
        &BTreeMap::from([(
            output,
            Rect {
                x: 0,
                y: 0,
                width: 2560,
                height: 1440,
            },
        )]),
        &BTreeMap::from([(
            output,
            Rect {
                x: 0,
                y: 0,
                width: 2560,
                height: 1440,
            },
        )]),
        chrome,
    )
    .unwrap();

    assert_eq!(reconciled.adjusted_surfaces, 0);
    assert_eq!(
        reconciled.policy.outputs[0].placements[0].geometry,
        geometry
    );
    assert_eq!(
        reconciled.policy.outputs[0].placements[0].requested_size,
        None
    );
    assert_eq!(
        reconciled.content[&surface].geometry,
        Rect {
            x: geometry.x + chrome.clearance(),
            y: geometry.y + chrome.clearance(),
            width: geometry.width - chrome.clearance() * 2,
            height: geometry.height - chrome.clearance() * 2,
        }
    );
    assert_eq!(
        reconciled.content[&surface].requested_size,
        Some(Size {
            width: geometry.width - chrome.clearance() * 2,
            height: geometry.height - chrome.clearance() * 2,
        })
    );
}

#[test]
fn public_policy_fullscreen_reconciliation_preserves_the_full_output() {
    let output = OutputId::from_raw(1);
    let surface = SurfaceId::new(10, 1);
    let sibling = SurfaceId::new(11, 1);
    let full = Rect {
        x: 0,
        y: 0,
        width: 2560,
        height: 1440,
    };
    let work = Rect {
        x: 0,
        y: sophia_engine::INDICATOR_STRIP_HEIGHT,
        width: 2560,
        height: 1440 - sophia_engine::INDICATOR_STRIP_HEIGHT,
    };
    let proposal = sophia_protocol::PolicyProjectionProposal {
        launch_contexts: Vec::new(),
        output_launch_contexts: Vec::new(),
        translation_groups: Vec::new(),
        tab_groups: Vec::new(),
        transaction: TransactionId::from_raw(11),
        connection_epoch: 1,
        request_id: 1,
        base_generation: 1,
        active_output: output,
        outputs: vec![sophia_protocol::PolicyOutputProjection {
            output,
            placements: vec![
                sophia_protocol::PolicySurfacePlacement {
                    surface,
                    surface_generation: 1,
                    geometry: full,
                    requested_size: Some(Size {
                        width: full.width,
                        height: full.height,
                    }),
                    crop: None,
                    transform: sophia_protocol::PolicyTransform::Identity,
                    presentation: sophia_protocol::PolicyPresentationState {
                        fullscreen: true,
                        ..Default::default()
                    },
                },
                sophia_protocol::PolicySurfacePlacement {
                    surface: sibling,
                    surface_generation: 1,
                    geometry: full,
                    requested_size: Some(Size {
                        width: full.width,
                        height: full.height,
                    }),
                    crop: None,
                    transform: sophia_protocol::PolicyTransform::Identity,
                    presentation: Default::default(),
                },
            ],
            focus: Some(surface),
        }],
        indicators: Vec::new(),
        output_statuses: Vec::new(),
    };

    let chrome = sophia_engine::SurfaceChromeStyle::default();
    let reconciled = reconcile_public_policy_proposal(
        &PersistentLiveLayout::default(),
        &proposal,
        &BTreeMap::from([(output, work)]),
        &BTreeMap::from([(output, full)]),
        chrome,
    )
    .unwrap();

    assert_eq!(reconciled.adjusted_surfaces, 1);
    let outer = &reconciled.policy.outputs[0].placements[0];
    assert_eq!(outer.geometry, full);
    assert_eq!(
        outer.requested_size,
        Some(Size {
            width: full.width,
            height: full.height,
        })
    );
    assert!(outer.presentation.fullscreen);
    assert_eq!(
        reconciled.content[&surface].geometry,
        Rect {
            x: chrome.clearance(),
            y: chrome.clearance(),
            width: full.width - chrome.clearance() * 2,
            height: full.height - chrome.clearance() * 2,
        }
    );
    let sibling_outer = &reconciled.policy.outputs[0].placements[1];
    assert_eq!(sibling_outer.geometry, work);
    assert_eq!(
        sibling_outer.requested_size,
        Some(Size {
            width: work.width,
            height: work.height,
        })
    );
}

#[test]
fn public_policy_materializes_reconciled_content_without_committing_content_to_the_reducer() {
    let output = OutputId::from_raw(1);
    let surface = SurfaceId::new(10, 1);
    let outer = Rect {
        x: 0,
        y: 0,
        width: 640,
        height: 480,
    };
    let mut layout = PersistentLiveLayout::default();
    layout.layers.insert(surface, test_layer(surface, outer));
    let proposal = sophia_protocol::PolicyProjectionProposal {
        launch_contexts: Vec::new(),
        output_launch_contexts: Vec::new(),
        translation_groups: Vec::new(),
        tab_groups: Vec::new(),
        transaction: TransactionId::from_raw(11),
        connection_epoch: 1,
        request_id: 2,
        base_generation: 3,
        active_output: output,
        outputs: vec![policy_projection(output, surface)],
        indicators: Vec::new(),
        output_statuses: Vec::new(),
    };
    let chrome = sophia_engine::SurfaceChromeStyle::default();
    let reconciled = reconcile_public_policy_proposal(
        &layout,
        &proposal,
        &BTreeMap::from([(output, outer)]),
        &BTreeMap::from([(output, outer)]),
        chrome,
    )
    .unwrap();
    let settlement = LivePolicySettlementIdentity {
        connection_epoch: 1,
        request_id: 2,
        scene_generation: 3,
        transaction: proposal.transaction,
        expect_session_operation: false,
        session_operation: false,
    };
    let live = public_live_proposal(
        &layout,
        output,
        reconciled.policy.outputs.clone(),
        proposal.transaction,
        LiveWmProposalSource::Manage(surface),
        settlement,
        &reconciled,
    )
    .unwrap();

    assert_eq!(reconciled.policy.outputs[0].placements[0].geometry, outer);
    assert_eq!(
        live.layers.last().unwrap().geometry,
        reconciled.content[&surface].geometry
    );
    assert_eq!(
        live.requested_sizes.get(&surface),
        reconciled.content[&surface].requested_size.as_ref(),
    );
}

/// The storm regression: policy answers a `Manage` request by placing nothing.
///
/// A monocle layout places one window however many it is shown, so this is an
/// ordinary answer rather than a failure. The owner must stop asking until the
/// facts change; re-asking every turn is what drove a physical session into a
/// page-flip hard stall.
#[test]
fn a_committed_manage_that_places_nothing_settles_until_the_facts_change() {
    let guide = SurfaceId::new(9, 1);
    let browser = SurfaceId::new(18, 1);
    let geometry = Rect {
        x: 0,
        y: 0,
        width: 640,
        height: 400,
    };
    let mut layout = PersistentLiveLayout::default();
    layout.observe_authority_batch(&manage_request_batch(browser, geometry, 1));
    assert_eq!(layout.next_unmanaged_surface(), Some(browser));

    // Policy commits a layout holding only the guide.
    let transaction = TransactionId::from_raw(10);
    let result = layout.commit_proposal(manage_proposal_without_placement(
        transaction,
        browser,
        guide,
        geometry,
        2,
    ));
    assert_eq!(result.source, Some(LiveWmProposalSource::Manage(browser)));
    assert_eq!(
        layout.next_unmanaged_surface(),
        None,
        "a settled answer stops the owner re-asking"
    );
    assert!(layout.unmanaged_surfaces.contains(&browser));
    assert!(layout.surface_requires_admission(browser));

    // Re-asking at the same facts stays settled, however many turns pass.
    let repeat = TransactionId::from_raw(11);
    layout.commit_proposal(manage_proposal_without_placement(
        repeat, browser, guide, geometry, 2,
    ));
    assert_eq!(layout.next_unmanaged_surface(), None);

    // Answering again at the same facts re-settles, so the owner still holds off.
    // What reopens the question is a commit carrying different facts without an
    // answer about this surface -- the shape a layout switch produces, since an
    // action-sourced proposal is not a `Manage` reply.
    let relayout = TransactionId::from_raw(12);
    let mut moved = manage_proposal_without_placement(relayout, browser, guide, geometry, 3);
    moved.source = Some(LiveWmProposalSource::Relayout);
    layout.commit_proposal(moved);
    assert_eq!(layout.next_unmanaged_surface(), Some(browser));
}

/// Changed authority facts re-arm the question without waiting for a commit.
#[test]
fn changed_surface_facts_reopen_a_settled_manage_answer() {
    let guide = SurfaceId::new(9, 1);
    let browser = SurfaceId::new(18, 1);
    let geometry = Rect {
        x: 0,
        y: 0,
        width: 640,
        height: 400,
    };
    let mut layout = PersistentLiveLayout::default();
    layout.observe_authority_batch(&manage_request_batch(browser, geometry, 1));
    layout.commit_proposal(manage_proposal_without_placement(
        TransactionId::from_raw(10),
        browser,
        guide,
        geometry,
        2,
    ));
    assert_eq!(layout.next_unmanaged_surface(), None);

    let mut resized = crate::live_session::wm_update_coordinator_batch(TransactionId::from_raw(11));
    resized.surface_presentations.push(
        sophia_x_authority::XAuthoritySurfacePresentationObservation {
            surface: browser,
            role: sophia_protocol::SurfacePresentationRole::PolicyManaged,
            kind: sophia_protocol::LayoutNodeKind::Toplevel,
            placement_preference: sophia_protocol::SurfacePlacementPreference::Default,
            owner: None,
            stack_rank: 0,
            mapped: true,
            geometry: Rect {
                x: 0,
                y: 0,
                width: 800,
                height: 600,
            },
            constraints: SurfaceConstraints {
                min_size: None,
                max_size: None,
            },
            generation: 2,
        },
    );
    layout.observe_authority_batch(&resized);

    assert_eq!(layout.next_unmanaged_surface(), Some(browser));
}

fn manage_request_batch(
    surface: SurfaceId,
    geometry: Rect,
    generation: u64,
) -> XAuthorityObservedTransactionBatch {
    let mut observed = crate::live_session::wm_update_coordinator_batch(TransactionId::from_raw(9));
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
            constraints: SurfaceConstraints {
                min_size: None,
                max_size: None,
            },
            generation,
        });
    observed
}

/// A `Manage(surface)` proposal that commits a layout excluding that surface.
fn manage_proposal_without_placement(
    transaction: TransactionId,
    surface: SurfaceId,
    placed: SurfaceId,
    geometry: Rect,
    scene_generation: u64,
) -> LiveWmProposal {
    LiveWmProposal {
        transaction,
        layers: vec![test_layer(placed, geometry)],
        requested_sizes: BTreeMap::new(),
        presentation_states: BTreeMap::new(),
        configure_deliveries: 0,
        focus: Some(placed),
        timeout: Duration::from_secs(1),
        update: sophia_engine::WmTransactionUpdate {
            commit: TransactionCommit {
                transaction,
                outcome: TransactionOutcome::Committed,
                applied_surfaces: vec![placed],
            },
        },
        moved_surfaces: 0,
        source: Some(LiveWmProposalSource::Manage(surface)),
        policy_settlement: Some(LivePolicySettlementIdentity {
            connection_epoch: 1,
            request_id: 1,
            scene_generation,
            transaction,
            expect_session_operation: false,
            session_operation: false,
        }),
    }
}

#[test]
fn committed_public_manage_consumes_planning_ownership_before_visual_retirement() {
    let surface = SurfaceId::new(9, 1);
    let geometry = Rect {
        x: 0,
        y: 0,
        width: 640,
        height: 400,
    };
    let transaction = TransactionId::from_raw(10);
    let mut layout = PersistentLiveLayout::default();
    let mut observed = crate::live_session::wm_update_coordinator_batch(TransactionId::from_raw(9));
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
            constraints: SurfaceConstraints {
                min_size: None,
                max_size: None,
            },
            generation: 1,
        });
    layout.observe_authority_batch(&observed);
    assert_eq!(layout.next_unmanaged_surface(), Some(surface));

    let result = layout.commit_proposal(LiveWmProposal {
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
        source: Some(LiveWmProposalSource::Manage(surface)),
        policy_settlement: Some(LivePolicySettlementIdentity {
            connection_epoch: 1,
            request_id: 1,
            scene_generation: 2,
            transaction,
            expect_session_operation: false,
            session_operation: false,
        }),
    });

    assert_eq!(result.source, Some(LiveWmProposalSource::Manage(surface)));
    assert_eq!(layout.next_unmanaged_surface(), None);
    assert!(layout.planning_surfaces.contains_key(&surface));
    assert!(layout.surface_requires_admission(surface));
}

fn hold_test_resize(
    layout: &mut PersistentLiveLayout,
    surface: SurfaceId,
    transaction: TransactionId,
    geometry: Rect,
) {
    layout.pending = Some(PendingLiveWmLayout {
        transaction,
        layers: vec![test_layer(surface, geometry)],
        requested_sizes: BTreeMap::from([(
            surface,
            Size {
                width: geometry.width,
                height: geometry.height,
            },
        )]),
        presentation_states: BTreeMap::new(),
        presentation_settlements: BTreeSet::new(),
        configure_deliveries: 0,
        focus: Some(surface),
        deadline: Instant::now() + Duration::from_secs(1),
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
}

#[test]
fn presented_resize_ignores_exact_backing_snapshot_until_present_retires() {
    let surface = SurfaceId::new(83, 1);
    let launch = Size {
        width: 1280,
        height: 1040,
    };
    let target = Size {
        width: 1276,
        height: 1422,
    };
    let target_geometry = Rect {
        x: 0,
        y: 0,
        width: target.width,
        height: target.height,
    };
    let mut layout = PersistentLiveLayout::default();
    layout.layout_epochs.record_safe_observation(
        dma_candidate(
            TransactionId::from_raw(830),
            surface,
            BufferHandle::from_raw(830),
        ),
        launch,
        sophia_engine::SurfaceVisualEvidence::PresentedBuffer,
    );
    layout.layout_epochs.record_committed(surface, launch);
    layout
        .layout_epochs
        .set_admission(surface, sophia_engine::SurfaceAdmissionState::Managed);
    layout.layout_epochs.set_pending_target(surface, target);
    hold_test_resize(
        &mut layout,
        surface,
        TransactionId::from_raw(831),
        target_geometry,
    );

    let backing_handle = 832;
    layout.cpu_buffer_sizes.insert(backing_handle, target);
    let backing_transaction = TransactionId::from_raw(832);
    let mut backing = crate::live_session::wm_update_coordinator_batch(backing_transaction);
    backing.transactions.push(SurfaceTransaction {
        input_region: None,
        transaction: backing_transaction,
        authority: AuthorityKind::SophiaX,
        surface,
        namespace: None,
        target_geometry,
        presentation_extent: Size {
            width: target_geometry.width,
            height: target_geometry.height,
        },
        content: sophia_protocol::SurfaceContentSet::singleton(
            BufferSource::CpuBuffer {
                handle: backing_handle,
            },
            sophia_protocol::Size {
                width: target_geometry.width,
                height: target_geometry.height,
            },
        ),

        damage: Region::single(target_geometry),
        readiness: SurfaceTransactionReadiness::Ready,
        timeout_msec: 250,
        previous_committed_generation: 1,
    });
    layout.observe_authority_batch(&backing);

    assert!(
        layout
            .pending
            .as_ref()
            .unwrap()
            .staged_transactions
            .is_empty()
    );
    assert!(layout.resolve_pending().is_none());
    assert_eq!(layout.layout_epochs.committed_size(surface), Some(launch));
    assert_eq!(layout.layout_epochs.pending_target(surface), Some(target));

    let present_buffer = BufferHandle::from_raw(833);
    layout.dma_buf_sizes.insert(present_buffer, target);
    let present_transaction = TransactionId::from_raw(833);
    let mut present = crate::live_session::wm_update_coordinator_batch(present_transaction);
    present.transactions.push(SurfaceTransaction {
        input_region: None,
        transaction: present_transaction,
        authority: AuthorityKind::SophiaX,
        surface,
        namespace: None,
        target_geometry,
        presentation_extent: Size {
            width: target_geometry.width,
            height: target_geometry.height,
        },
        content: sophia_protocol::SurfaceContentSet::singleton(
            BufferSource::DmaBuf {
                handle: present_buffer.raw(),
            },
            sophia_protocol::Size {
                width: target_geometry.width,
                height: target_geometry.height,
            },
        ),

        damage: Region::single(target_geometry),
        readiness: SurfaceTransactionReadiness::Ready,
        timeout_msec: 250,
        previous_committed_generation: 1,
    });
    present
        .present_submissions
        .push(sophia_x_authority::XAuthorityPresentSubmission {
            transaction: present_transaction,
            surface,
            buffer: present_buffer,
            x_offset: 0,
            y_offset: 0,
            acquire_fence: None,
            idle_fence: None,
        });
    layout.observe_authority_batch(&present);

    assert!(layout.resolve_pending().is_some());
    assert!(layout.awaiting_visual_commits.surface_awaiting(surface));
    assert_eq!(layout.layout_epochs.committed_size(surface), Some(launch));
    assert_eq!(layout.layout_epochs.pending_target(surface), Some(target));
    assert!(layout.complete_visual_commit(
        dma_candidate(present_transaction, surface, present_buffer),
        target,
    ));
    assert_eq!(layout.layout_epochs.committed_size(surface), Some(target));
    assert_eq!(layout.layout_epochs.pending_target(surface), None);
}

#[test]
fn backing_resize_still_commits_for_cpu_only_surface() {
    let surface = SurfaceId::new(84, 1);
    let launch = Size {
        width: 640,
        height: 480,
    };
    let target = Size {
        width: 800,
        height: 600,
    };
    let target_geometry = Rect {
        x: 0,
        y: 0,
        width: target.width,
        height: target.height,
    };
    let mut layout = PersistentLiveLayout::default();
    layout.layout_epochs.record_committed(surface, launch);
    layout
        .layout_epochs
        .set_admission(surface, sophia_engine::SurfaceAdmissionState::Managed);
    layout.layout_epochs.set_pending_target(surface, target);
    hold_test_resize(
        &mut layout,
        surface,
        TransactionId::from_raw(840),
        target_geometry,
    );

    let buffer = 841;
    layout.cpu_buffer_sizes.insert(buffer, target);
    let transaction = TransactionId::from_raw(841);
    let mut backing = crate::live_session::wm_update_coordinator_batch(transaction);
    backing.transactions.push(SurfaceTransaction {
        input_region: None,
        transaction,
        authority: AuthorityKind::SophiaX,
        surface,
        namespace: None,
        target_geometry,
        presentation_extent: Size {
            width: target_geometry.width,
            height: target_geometry.height,
        },
        content: sophia_protocol::SurfaceContentSet::singleton(
            BufferSource::CpuBuffer { handle: buffer },
            sophia_protocol::Size {
                width: target_geometry.width,
                height: target_geometry.height,
            },
        ),

        damage: Region::single(target_geometry),
        readiness: SurfaceTransactionReadiness::Ready,
        timeout_msec: 250,
        previous_committed_generation: 1,
    });
    layout.observe_authority_batch(&backing);

    assert!(layout.resolve_pending().is_some());
    assert!(!layout.awaiting_visual_commits.surface_awaiting(surface));
    assert_eq!(layout.layout_epochs.committed_size(surface), Some(target));
    assert_eq!(layout.layout_epochs.pending_target(surface), None);
}

#[test]
fn recovery_content_extent_stays_behind_the_wm_policy_boundary() {
    let surface = SurfaceId::new(3, 1);
    let workspace = sophia_protocol::WorkspaceId::from_raw(1);
    let content = Size {
        width: 500,
        height: 500,
    };
    let mut epochs = sophia_engine::LayoutEpochCoordinator::default();
    epochs.record_committed(surface, content);
    epochs
        .begin_recovery(
            [(
                surface,
                Size {
                    width: 1276,
                    height: 1422,
                },
            )],
            [surface],
        )
        .unwrap();
    let style = sophia_engine::SurfaceChromeStyle {
        frame: sophia_engine::SurfaceFrameStyle {
            width: 2,
            ..sophia_engine::SurfaceFrameStyle::default()
        },
        ..sophia_engine::SurfaceChromeStyle::default()
    };

    let node = test_live_layout_node(
        &test_layer(
            surface,
            Rect {
                x: 2,
                y: 2,
                width: content.width,
                height: content.height,
            },
        ),
        workspace,
        &epochs,
        style,
    )
    .unwrap();

    assert_eq!(node.constraints.min_size, None);
    assert_eq!(node.constraints.max_size, None);
    assert!(node.capabilities.resizable);
    assert_eq!(
        node.geometry,
        Rect {
            x: 0,
            y: 0,
            width: 504,
            height: 504,
        }
    );
}

#[test]
fn declared_fixed_extent_still_crosses_the_wm_policy_boundary() {
    let surface = SurfaceId::new(30, 1);
    let workspace = sophia_protocol::WorkspaceId::from_raw(1);
    let fixed = Size {
        width: 500,
        height: 500,
    };
    let mut epochs = sophia_engine::LayoutEpochCoordinator::default();
    epochs.set_declared_constraints(
        surface,
        SurfaceConstraints {
            min_size: Some(fixed),
            max_size: Some(fixed),
        },
    );
    let style = sophia_engine::SurfaceChromeStyle::default();
    let node = test_live_layout_node(
        &test_layer(
            surface,
            Rect {
                x: 0,
                y: 0,
                width: fixed.width,
                height: fixed.height,
            },
        ),
        workspace,
        &epochs,
        style,
    )
    .unwrap();

    let outer = sophia_engine::outer_surface_constraints(
        SurfaceConstraints {
            min_size: Some(fixed),
            max_size: Some(fixed),
        },
        style,
    )
    .unwrap();
    assert_eq!(node.constraints, outer);
    assert!(!node.capabilities.resizable);
}

#[test]
fn presentation_request_produces_a_wm_node_before_pixels_exist() {
    let surface = SurfaceId::new(4, 1);
    let geometry = Rect {
        x: 80,
        y: 60,
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
    let mut batch = crate::live_session::wm_update_coordinator_batch(TransactionId::from_raw(10));
    let client = sophia_x_authority::XServerFrontendClientId::from_raw(1);
    batch.client = Some(client);
    add_test_surface_route(&mut batch, surface, client);
    batch.presentation_intents.push(intent);
    let mut layout = PersistentLiveLayout::default();

    let observation = layout.observe_authority_batch(&batch);

    assert_eq!(observation.new_surfaces, vec![surface]);
    assert_eq!(layout.next_unmanaged_surface(), Some(surface));
    assert!(layout.layers.is_empty());
    assert_eq!(
        planning_layers_for(&layout, [surface])[0].source,
        BufferSource::None
    );
    let chrome = sophia_engine::SurfaceChromeStyle::default();
    let node = test_layout_node_from_facts(
        layout.layout_facts(surface).unwrap(),
        WorkspaceId::from_raw(1),
        &layout.layout_epochs,
        chrome,
    )
    .unwrap();
    assert_eq!(node.surface, surface);
    assert_eq!(
        node.geometry,
        sophia_engine::outer_surface_geometry(geometry, chrome).unwrap()
    );
}

/// A surface on any output belongs to the scene, not only one on the primary.
///
/// The presentation layout spans the whole desktop, so this query decides
/// whether a layer is composed at all. Asked about one output it answered a
/// narrower question, and on a mixed topology the extended output's surface
/// left the scene entirely: its Present then never became visible, never
/// retired, and never committed the resize that placed it there.
#[test]
fn committed_projections_place_a_surface_on_any_of_its_outputs() {
    let primary = OutputId::from_raw(1);
    let extended = OutputId::from_raw(2);
    let mirrored = SurfaceId::new(11, 1);
    let placed = SurfaceId::new(12, 1);
    let projections = vec![
        policy_projection(primary, mirrored),
        policy_projection(extended, placed),
    ];

    assert!(policy_projections_place_surface(
        &projections,
        &[primary, extended],
        placed
    ));
    assert!(policy_projections_place_surface(
        &projections,
        &[primary, extended],
        mirrored
    ));
    // The narrow question, which is the one that used to be asked.
    assert!(!policy_projections_place_surface(
        &projections,
        &[primary],
        placed
    ));
    // An output that is no longer live cannot keep a surface in the scene.
    assert!(!policy_projections_place_surface(
        &projections,
        &[primary],
        SurfaceId::new(13, 1)
    ));
}

/// A pointer gesture asks which output's committed projection places its
/// surface. When none does -- the surface mapped and is not yet laid out, or a
/// scrolling layout holds it out of view, or it sits between the transaction
/// that removed it and the one that returns it -- the answer is "nowhere", and
/// the gesture is dropped. It used to be an error, and the owner loop treats
/// an error as fatal: a Super+button on a freshly tiled column ended the
/// operator's whole desktop (2026-09-23, release bf43425d).
#[test]
fn a_pointer_gesture_on_an_unplaced_surface_goes_nowhere_rather_than_failing() {
    let primary = OutputId::from_raw(1);
    let extended = OutputId::from_raw(2);
    let placed = SurfaceId::new(12, 1);
    let unplaced = SurfaceId::new(13, 1);
    let projections = vec![
        policy_projection(primary, SurfaceId::new(11, 1)),
        policy_projection(extended, placed),
    ];

    assert_eq!(
        committed_output_placing(&projections, placed),
        Some(extended)
    );
    assert_eq!(committed_output_placing(&projections, unplaced), None);
    assert_eq!(committed_output_placing(&[], placed), None);

    // The dropped gesture leaves a retained trace that names why.
    let record = "sophia_live_wm_pointer schema=2 status=interaction_dropped reason=target_unplaced phase=Begin mode=Move surface=13";
    let reduced = crate::diagnostics::reduced_record(record).unwrap();
    assert!(
        reduced.contains("status=interaction_dropped"),
        "reduction lost the drop: {reduced}"
    );
    assert!(
        reduced.contains("surface=13"),
        "reduction lost the surface: {reduced}"
    );
}

fn policy_projection(
    output: OutputId,
    surface: SurfaceId,
) -> sophia_protocol::PolicyOutputProjection {
    sophia_protocol::PolicyOutputProjection {
        output,
        placements: vec![sophia_protocol::PolicySurfacePlacement {
            surface,
            surface_generation: 1,
            geometry: Rect {
                x: 0,
                y: 0,
                width: 640,
                height: 480,
            },
            requested_size: None,
            crop: None,
            transform: sophia_protocol::PolicyTransform::Identity,
            presentation: sophia_protocol::PolicyPresentationState::default(),
        }],
        focus: None,
    }
}

include!("wm_session_tests/admission.rs");
include!("wm_session_tests/direct_map.rs");
include!("wm_session_tests/geometry.rs");
include!("wm_session_tests/pre_admission.rs");
include!("wm_session_tests/recovery.rs");

#[path = "../../../tests/support/policy_output_ownership.rs"]
mod policy_output_ownership;

#[path = "../../../tests/support/work_area_recovery.rs"]
mod work_area_recovery;

#[path = "../../../tests/support/withdrawn_policy_snapshot.rs"]
mod withdrawn_policy_snapshot;
