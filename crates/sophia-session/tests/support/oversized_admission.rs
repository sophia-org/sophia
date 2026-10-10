use super::*;

#[test]
fn an_observed_extent_without_retained_pixels_cannot_widen_the_output() {
    let surface = SurfaceId::new(91, 1);
    let geometry = Rect {
        x: 0,
        y: 0,
        width: 2542,
        height: 1398,
    };
    let mut layout = PersistentLiveLayout::default();
    layout.observe_authority_batch(&map_batch(surface, 1, geometry));
    let mut first = present_batch(surface, 2, 44, geometry, false);
    first.surface_presentations.clear();
    layout.dma_buf_sizes.insert(
        sophia_protocol::BufferHandle::from_raw(44),
        Size {
            width: geometry.width,
            height: geometry.height,
        },
    );
    layout.observe_authority_batch(&first);
    assert_eq!(
        layout.retained_admission_candidate(surface),
        Some(first.transactions[0].key())
    );

    layout.pre_admission_groups.clear();
    assert_eq!(layout.retained_admission_candidate(surface), None);
    let bounded = proposal(&layout, surface, 3, LiveWmProposalSource::Manage(surface));
    assert!(bounded.layers[0].geometry.width <= 1920);
    assert!(bounded.layers[0].geometry.height <= 1056);
}

fn proposal(
    layout: &PersistentLiveLayout,
    surface: SurfaceId,
    epoch: u64,
    source: LiveWmProposalSource,
) -> LiveWmProposal {
    let output = OutputId::from_raw(2);
    let bounds = Rect {
        x: 2560,
        y: 24,
        width: 1920,
        height: 1056,
    };
    let transaction = TransactionId::from_raw(epoch);
    let projection = sophia_protocol::PolicyProjectionProposal {
        transaction,
        connection_epoch: 1,
        request_id: epoch,
        base_generation: 1,
        active_output: output,
        outputs: vec![sophia_protocol::PolicyOutputProjection {
            output,
            placements: vec![sophia_protocol::PolicySurfacePlacement {
                surface,
                surface_generation: 1,
                geometry: bounds,
                requested_size: Some(Size {
                    width: bounds.width,
                    height: bounds.height,
                }),
                crop: None,
                transform: sophia_protocol::PolicyTransform::Identity,
                presentation: sophia_protocol::PolicyPresentationState::default(),
            }],
            focus: Some(surface),
        }],
        presentation: None,
        launch_contexts: vec![],
        output_launch_contexts: vec![],
        translation_groups: vec![],
        tab_groups: vec![],
        indicators: vec![],
        output_statuses: vec![],
    };
    let reconciliation = reconcile_public_policy_proposal(
        layout,
        &projection,
        &BTreeMap::from([(output, bounds)]),
        &BTreeMap::from([(output, bounds)]),
        sophia_engine::SurfaceChromeStyle::default(),
    )
    .unwrap();
    public_live_proposal(
        layout,
        output,
        reconciliation.policy.outputs.clone(),
        transaction,
        source,
        LivePolicySettlementIdentity {
            connection_epoch: 1,
            request_id: epoch,
            scene_generation: 1,
            transaction,
            expect_session_operation: false,
            session_operation: false,
        },
        &reconciliation,
    )
    .unwrap()
}

#[test]
fn an_oversized_first_frame_retires_before_requesting_the_smaller_output_size() {
    let surface = SurfaceId::new(91, 1);
    let initial = Rect {
        x: 0,
        y: 0,
        width: 2542,
        height: 1398,
    };
    let initial_size = Size {
        width: initial.width,
        height: initial.height,
    };
    let mut layout = PersistentLiveLayout::default();
    layout.observe_authority_batch(&map_batch(surface, 1, initial));
    let mut controls = crate::session_control::SessionControlQueue::default();
    let launch = proposal(&layout, surface, 2, LiveWmProposalSource::Manage(surface));
    let desired = launch.requested_sizes[&surface];
    assert!(layout.stage(launch, &mut controls).unwrap().is_none());
    assert!(layout.acknowledge_admission_control(TransactionId::from_raw(2), surface));
    assert!(layout.acknowledge_presentation_control(TransactionId::from_raw(2), surface));
    assert!(layout.resolve_pending().is_some());
    assert_eq!(layout.layout_epochs.pending_target(surface), Some(desired));
    // The owner loop consumes the policy-only admission's focus request.
    layout.focus_to_apply.take();

    // The client waits for this Present's completion before drawing again.
    // Map and admission already settled, but this larger frame is the only
    // complete content in custody. A configure cannot manufacture a successor.
    let mut first = present_batch(surface, 3, 44, initial, false);
    first.surface_presentations.clear();
    layout
        .dma_buf_sizes
        .insert(sophia_protocol::BufferHandle::from_raw(44), initial_size);
    let candidate = first.transactions[0].key();
    layout.observe_authority_batch(&first);
    assert_eq!(
        layout.layout_epochs.recovery_extent(surface),
        Some(initial_size)
    );
    let recovery = proposal(&layout, surface, 4, LiveWmProposalSource::Relayout);
    let result = layout
        .stage(recovery, &mut controls)
        .unwrap()
        .or_else(|| layout.resolve_pending());
    assert!(
        result.is_some(),
        "first-frame admission waited for a smaller frame before retiring the retained one"
    );
    assert!(layout.pending.is_none());
    assert!(
        layout
            .awaiting_visual_commits
            .exact_candidate(candidate, initial_size)
    );
    assert_eq!(layout.layers[&surface].output, Some(OutputId::from_raw(2)));
    assert_eq!(layout.layers[&surface].geometry.width, initial_size.width);
    assert_eq!(layout.layers[&surface].geometry.height, initial_size.height);
    assert_eq!(layout.layout_epochs.pending_target(surface), Some(desired));
    assert!(
        layout.focus_to_apply.is_none(),
        "policy admission is not presentation"
    );

    let reflow = proposal(&layout, surface, 5, LiveWmProposalSource::Relayout);
    assert!(layout.stage(reflow, &mut controls).unwrap().is_some());
    assert!(
        layout.pending.is_none(),
        "an in-flight admission keeps its size"
    );
    assert!(
        layout
            .awaiting_visual_commits
            .exact_candidate(candidate, initial_size)
    );
    assert_eq!(layout.layers[&surface].output, Some(OutputId::from_raw(2)));
    assert!(layout.focus_to_apply.is_none());

    let wrong = sophia_protocol::SurfaceTransactionKey {
        transaction: TransactionId::from_raw(99),
        ..candidate
    };
    assert!(!layout.complete_admission_retirement(wrong));
    assert!(layout.complete_visual_commit(candidate, initial_size));
    assert!(layout.complete_admission_retirement(candidate));
    assert!(!layout.complete_admission_retirement(candidate));
    assert_eq!(layout.layout_epochs.recovery_extent(surface), None);
    assert_eq!(layout.layout_epochs.pending_target(surface), Some(desired));
    assert_eq!(
        layout.focus_to_apply,
        Some((TransactionId::from_raw(5), surface))
    );

    let resize = proposal(&layout, surface, 6, LiveWmProposalSource::Relayout);
    assert_eq!(resize.requested_sizes[&surface], desired);
    assert!(layout.stage(resize, &mut controls).unwrap().is_none());
    let mut successor = present_batch(
        surface,
        7,
        45,
        Rect {
            width: desired.width,
            height: desired.height,
            ..initial
        },
        false,
    );
    successor.surface_presentations.clear();
    layout
        .dma_buf_sizes
        .insert(sophia_protocol::BufferHandle::from_raw(45), desired);
    let successor_key = successor.transactions[0].key();
    layout.observe_authority_batch(&successor);
    assert!(layout.resolve_pending().is_some());
    assert!(layout.complete_visual_commit(successor_key, desired));
    assert_eq!(layout.layout_epochs.pending_target(surface), None);
}
